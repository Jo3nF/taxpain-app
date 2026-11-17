//! RETA por ingresos reales: tramos de rendimientos, bases de cotización, rendimiento neto
//! computable del autónomo societario, regularización anual de la TGSS y ventanas para cambiar
//! la base. Cálculo puro (sin IO): lo usa `taxpain-app::autonomo_societario`.
//!
//! Norma (cotejada con el BOE el 2026-11-16; tabla en `docs/revisiones/2026-11-verificacion-boe.md`):
//! - Art. 308.1 TRLGSS (RDLeg 8/2015, BOE-A-2015-11724, texto consolidado a 11-11-2026), en la
//!   redacción del RDL 13/2022 (BOE-A-2022-12482): cotización en función de los rendimientos
//!   netos anuales; base elegida entre la mínima y la máxima del tramo que corresponde al
//!   promedio mensual de los rendimientos previstos; regularización al año siguiente con los
//!   rendimientos que comunica la AEAT (a ingresar si la cotización fue inferior a la de la base
//!   mínima del tramo; devolución de oficio si fue superior a la de la máxima: letra c, regla
//!   4.ª).
//! - Autónomos societarios (art. 305.2.b TRLGSS): computan «la totalidad de los rendimientos
//!   íntegros, dinerarios o en especie, derivados de la participación en los fondos propios» de
//!   las entidades en las que tengan, en la fecha de devengo del Impuesto sobre Sociedades, una
//!   participación igual o superior al 33 % del capital, o al 25 % si son administradores, y «la
//!   totalidad de los rendimientos de trabajo derivados de su actividad en dichas entidades»
//!   (art. 308.1.c, regla 1.ª, último párrafo). Por eso los dividendos entran al 100 %; con una
//!   participación menor no entran (el usuario pone el 0 %).
//! - Deducción por gastos genéricos: 7 %, y 3 % para los de las letras b) y e) del art. 305.2
//!   (art. 308.1.c, regla 2.ª). Para el 3 % basta con haber figurado noventa días de alta como
//!   societario en el año que se regulariza.
//! - Tablas: 2025, DT 1.ª del RDL 13/2022 y art. 18 de la Orden PJC/178/2025 (BOE-A-2025-3780);
//!   2026, art. 3.4 del RDL 3/2026 (BOE-A-2026-2548: la tabla de 2025, con la base máxima de los
//!   tramos 11 y 12 al tope del año) y art. 18 de la Orden PJC/297/2026 (BOE-A-2026-7296). 2027:
//!   sin norma publicada a 2026-11-16; se deja la de 2026 y se dice en `pendiente`.
//! - Tipo: 28,30 % de contingencias comunes, 1,30 % de profesionales, 0,90 % de cese de
//!   actividad, 0,10 % de formación profesional (arts. 18 y 37 de la orden de cada año) y el
//!   mecanismo de equidad intergeneracional de la DT 43.ª TRLGSS (0,80 % en 2025, 0,90 % en 2026,
//!   1,00 % en 2027).
//! - Base mínima de los societarios y de los familiares colaboradores: 1.000 € en 2023-2025 (DT
//!   7.ª del RDL 13/2022; art. 18.4 de la Orden PJC/178/2025); desde 2026, la base mínima del
//!   grupo 7 del Régimen General (art. 308.1.a, regla 4.ª, TRLGSS; art. 18.4 de la Orden
//!   PJC/297/2026): 1.424,40 € en 2026 (art. 3 de la orden). También exige noventa días de alta
//!   como societario en el año.
//! - Cambios de base: hasta seis al año, con efectos el día 1 del mes siguiente a cada bimestre
//!   en que se pide (art. 45.1 del Reglamento general de cotización, RD 2064/1995,
//!   BOE-A-1996-1579, en la redacción del RDL 13/2022; el art. 43 bis que lo regulaba antes está
//!   derogado desde el 1-1-2023).
//!
//! Lo que queda sin norma (2027) lo dice cada tabla en `pendiente`, y la pantalla lo muestra.

use serde::Serialize;
use time::{Date, Month};

use crate::money::{Money, Rate};

/// Tabla reducida (rendimientos inferiores al SMI) o general.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Tabla {
    Reducida,
    General,
}

/// Un tramo de rendimientos netos mensuales con su base mínima y máxima.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Tramo {
    pub tabla: Tabla,
    /// Número del tramo dentro de su tabla (1, 2, 3…).
    pub numero: u8,
    /// Rendimiento neto mensual desde el que se aplica (el anterior tramo acaba aquí).
    pub desde: Money,
    /// Hasta este rendimiento mensual, incluido; `None` en el último.
    pub hasta: Option<Money>,
    pub base_min: Money,
    pub base_max: Money,
}

impl Tramo {
    /// «General 7» o «Reducida 2».
    pub fn nombre(&self) -> String {
        let t = match self.tabla {
            Tabla::Reducida => "Reducida",
            Tabla::General => "General",
        };
        format!("{t} {}", self.numero)
    }
}

/// Tabla anual de tramos y bases del RETA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TablaAnual {
    pub year: i32,
    pub tramos: &'static [Tramo],
    /// Base mínima de los autónomos societarios (art. 305.2.b y e TRLGSS).
    pub base_min_societarios: Money,
    /// Tipo total de cotización (contingencias comunes y profesionales, cese de actividad,
    /// formación profesional y mecanismo de equidad intergeneracional).
    pub tipo: Rate,
    /// Norma de la que salen las cifras.
    pub norma: &'static str,
    /// Lo que falta cotejar con el BOE (vacío si está todo verificado).
    pub pendiente: &'static [&'static str],
}

const fn eur(e: i64, c: i64) -> Money {
    Money::from_cents(e * 100 + c)
}

const fn tr(
    tabla: Tabla,
    numero: u8,
    desde: Money,
    hasta: Option<Money>,
    min: Money,
    max: Money,
) -> Tramo {
    Tramo {
        tabla,
        numero,
        desde,
        hasta,
        base_min: min,
        base_max: max,
    }
}

use Tabla::{General, Reducida};

/// Tramos de 2025 (DT 1.ª.2 del RDL 13/2022, tabla de 2025), que el art. 3.4 del RDL 3/2026
/// mantiene en 2026. La base máxima de los tramos 11 y 12 de la tabla general es el tope del
/// año: 4.909,50 € en 2025 (art. 18.1 de la Orden PJC/178/2025) y 5.101,20 € en 2026 (art. 18.1
/// de la Orden PJC/297/2026).
const fn tramos(base_maxima: Money) -> [Tramo; 15] {
    [
        tr(
            Reducida,
            1,
            eur(0, 0),
            Some(eur(670, 0)),
            eur(653, 59),
            eur(718, 94),
        ),
        tr(
            Reducida,
            2,
            eur(670, 0),
            Some(eur(900, 0)),
            eur(718, 95),
            eur(900, 0),
        ),
        tr(
            Reducida,
            3,
            eur(900, 0),
            Some(eur(1166, 69)),
            eur(849, 67),
            eur(1166, 70),
        ),
        tr(
            General,
            1,
            eur(1166, 69),
            Some(eur(1300, 0)),
            eur(950, 98),
            eur(1300, 0),
        ),
        tr(
            General,
            2,
            eur(1300, 0),
            Some(eur(1500, 0)),
            eur(960, 78),
            eur(1500, 0),
        ),
        tr(
            General,
            3,
            eur(1500, 0),
            Some(eur(1700, 0)),
            eur(960, 78),
            eur(1700, 0),
        ),
        tr(
            General,
            4,
            eur(1700, 0),
            Some(eur(1850, 0)),
            eur(1143, 79),
            eur(1850, 0),
        ),
        tr(
            General,
            5,
            eur(1850, 0),
            Some(eur(2030, 0)),
            eur(1209, 15),
            eur(2030, 0),
        ),
        tr(
            General,
            6,
            eur(2030, 0),
            Some(eur(2330, 0)),
            eur(1274, 51),
            eur(2330, 0),
        ),
        tr(
            General,
            7,
            eur(2330, 0),
            Some(eur(2760, 0)),
            eur(1356, 21),
            eur(2760, 0),
        ),
        tr(
            General,
            8,
            eur(2760, 0),
            Some(eur(3190, 0)),
            eur(1437, 91),
            eur(3190, 0),
        ),
        tr(
            General,
            9,
            eur(3190, 0),
            Some(eur(3620, 0)),
            eur(1519, 61),
            eur(3620, 0),
        ),
        tr(
            General,
            10,
            eur(3620, 0),
            Some(eur(4050, 0)),
            eur(1601, 31),
            eur(4050, 0),
        ),
        tr(
            General,
            11,
            eur(4050, 0),
            Some(eur(6000, 0)),
            eur(1732, 3),
            base_maxima,
        ),
        tr(General, 12, eur(6000, 0), None, eur(1928, 10), base_maxima),
    ]
}

const TRAMOS_2025: [Tramo; 15] = tramos(eur(4909, 50));
const TRAMOS_2026: [Tramo; 15] = tramos(eur(5101, 20));

const PENDIENTE_2027: [&str; 4] = [
    "la tabla de 2027: a 16-11-2026 no hay ley de presupuestos ni orden de cotización de 2027; \
     aquí es la de 2026 (art. 3.4 del RDL 3/2026), que a su vez es la de 2025",
    "base máxima de 2027: se deja la de 2026 (5.101,20 €); la fijará la norma del año (art. 19.3 \
     TRLGSS)",
    "tipo total del 31,60 %: el MEI del 1,00 % sí está en la DT 43.ª TRLGSS; el resto de los \
     tipos (28,30 + 1,30 + 0,90 + 0,10) son los de 2026 y falta la orden de 2027",
    "base mínima de los societarios en 2027: será la mínima del grupo 7 del Régimen General de \
     2027 (art. 308.1.a, regla 4.ª, TRLGSS), que depende del salario mínimo de 2027; se deja la \
     de 2026 (1.424,40 €)",
];

/// Tablas conocidas, por año. Cada año hasta `parametros::ULTIMO_EJERCICIO_PREVISTO` tiene la
/// suya, aunque sea la del anterior prorrogada (lo dice `pendiente`).
pub const TABLAS: [TablaAnual; 3] = [
    TablaAnual {
        year: 2025,
        tramos: &TRAMOS_2025,
        base_min_societarios: eur(1000, 0),
        tipo: Rate::from_bp(3140),
        norma: "RDL 13/2022, DT 1.ª (tabla de 2025) y DT 7.ª; Orden PJC/178/2025, arts. 18 y 37 \
                (BOE-A-2025-3780)",
        pendiente: &[],
    },
    TablaAnual {
        year: 2026,
        tramos: &TRAMOS_2026,
        base_min_societarios: eur(1424, 40),
        tipo: Rate::from_bp(3150),
        norma: "RDL 3/2026, art. 3 (BOE-A-2026-2548); Orden PJC/297/2026, arts. 3, 18 y 37 \
                (BOE-A-2026-7296)",
        pendiente: &[],
    },
    TablaAnual {
        year: 2027,
        tramos: &TRAMOS_2026,
        base_min_societarios: eur(1424, 40),
        tipo: Rate::from_bp(3160),
        norma: "Art. 308 y DT 43.ª TRLGSS; sin ley de presupuestos ni orden de cotización de \
                2027: tabla de 2026 prorrogada",
        pendiente: &PENDIENTE_2027,
    },
];

/// Tabla del año, o la del último año conocido si es posterior (marcada como pendiente).
pub fn tabla(year: i32) -> Option<&'static TablaAnual> {
    TABLAS.iter().find(|t| t.year == year).or_else(|| {
        TABLAS
            .iter()
            .filter(|t| t.year < year)
            .max_by_key(|t| t.year)
    })
}

/// Tramo que corresponde a un rendimiento neto mensual (los negativos van al primero).
pub fn tramo_de(t: &TablaAnual, mensual: Money) -> &'static Tramo {
    t.tramos
        .iter()
        .find(|tr| tr.hasta.is_none_or(|h| mensual <= h))
        .unwrap_or(&t.tramos[t.tramos.len() - 1])
}

/// Deducción por gastos genéricos de los autónomos societarios (art. 308.1.c, regla 2.ª,
/// TRLGSS: «en que la deducción será del 3 por ciento»; basta con noventa días de alta como
/// societario en el año).
pub const DEDUCCION_SOCIETARIOS: Rate = Rate::percent(3);
/// Deducción por gastos genéricos del resto de autónomos (misma regla 2.ª).
pub const DEDUCCION_GENERAL: Rate = Rate::percent(7);

/// Días de alta como autónomo societario en el año que exigen la deducción del 3 % (art.
/// 308.1.c, regla 2.ª, TRLGSS) y la base mínima de los societarios (art. 308.1.a, regla 4.ª, y
/// DT 7.ª del RDL 13/2022): «noventa días» como poco.
pub const DIAS_ALTA_SOCIETARIO: u16 = 90;

/// Días de alta como societario dentro de `year`.
///
/// Con fecha de alta en el año, desde ese día hasta el 31 de diciembre (ambos incluidos); con
/// una fecha posterior al año, cero. Sin fecha, o con una anterior al año, el alta viene de antes:
/// el año entero si tiene los doce meses de alta y, si no, treinta días por mes (estimación: la
/// ficha no guarda la fecha de la baja).
pub fn dias_alta_societario(year: i32, alta: Option<Date>, meses: u8) -> u16 {
    let (Ok(inicio), Ok(fin)) = (
        Date::from_calendar_date(year, Month::January, 1),
        Date::from_calendar_date(year, Month::December, 31),
    ) else {
        return 0;
    };
    let dias_del_año = (fin - inicio).whole_days() as u16 + 1;
    match alta {
        Some(a) if a > fin => 0,
        Some(a) if a > inicio => (fin - a).whole_days() as u16 + 1,
        _ if meses >= 12 => dias_del_año,
        _ => u16::from(meses.max(1)) * 30,
    }
}

/// ¿Cumple los noventa días de alta como societario en el año?
pub fn cumple_dias_societario(dias: u16) -> bool {
    dias >= DIAS_ALTA_SOCIETARIO
}

/// Lo que entra en el rendimiento neto computable del año.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct EntradaRendimiento {
    /// Retribuciones dinerarias de la sociedad (brutas).
    pub dinerarias: Money,
    /// Retribuciones en especie de la sociedad (p. ej. la cuota del RETA que paga ella).
    pub especie: Money,
    /// Gastos deducibles de esas retribuciones, sin la cuota del RETA (art. 308.1.c: las
    /// cuotas no restan).
    pub gastos: Money,
    /// Dividendos íntegros cobrados de la sociedad.
    pub dividendos: Money,
    /// Parte de los dividendos que se computa: el 100 % si el socio tiene al menos el 33 % del
    /// capital, o el 25 % siendo administrador, en la fecha de devengo del IS (art. 308.1.c,
    /// regla 1.ª, último párrafo, TRLGSS); si no, el 0 %.
    pub pct_dividendos: Rate,
    /// Rendimientos netos de otras actividades (fuera de la sociedad), del año.
    pub otros: Money,
    /// Autónomo societario con noventa días de alta como tal en el año (deducción del 3 %: art.
    /// 308.1.c, regla 2.ª). Sin los noventa días, `false`: deducción del 7 % como el resto
    /// (ver `cumple_dias_societario`).
    pub societario: bool,
    /// Meses de alta en el año (1 a 12).
    pub meses: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rendimiento {
    pub dividendos_computados: Money,
    /// Retribuciones + dividendos computados + otros − gastos.
    pub neto: Money,
    pub deduccion_pct: Rate,
    pub deduccion: Money,
    /// Rendimiento neto computable del año.
    pub computable: Money,
    /// Promedio mensual (entre los meses de alta). El art. 308.1.c, regla 3.ª, dice que el
    /// rendimiento «se distribuirá proporcionalmente en el período a regularizar»; `TODO(VERIFY)`:
    /// la ley no dice si por días o por meses (lo remite al reglamento) y aquí es por meses.
    pub mensual: Money,
}

/// Rendimiento neto computable del año. La deducción genérica sólo se aplica si es positivo.
pub fn rendimiento(e: &EntradaRendimiento) -> Rendimiento {
    let dividendos_computados = e.dividendos.apply_rate(e.pct_dividendos);
    let neto = e.dinerarias + e.especie + dividendos_computados + e.otros - e.gastos;
    let deduccion_pct = if e.societario {
        DEDUCCION_SOCIETARIOS
    } else {
        DEDUCCION_GENERAL
    };
    let deduccion = if neto.is_negative() {
        Money::ZERO
    } else {
        neto.apply_rate(deduccion_pct)
    };
    let computable = neto - deduccion;
    let meses = i64::from(e.meses.clamp(1, 12));
    let c = computable.cents();
    // Redondeo al céntimo, la mitad hacia fuera.
    let mensual = Money::from_cents((c + c.signum() * meses / 2) / meses);
    Rendimiento {
        dividendos_computados,
        neto,
        deduccion_pct,
        deduccion,
        computable,
        mensual,
    }
}

/// Cuota mensual de una base.
pub fn cuota(base: Money, tipo: Rate) -> Money {
    base.apply_rate(tipo)
}

/// Bases mínima y máxima que valen con un tramo (la mínima de los societarios no baja de la
/// específica).
pub fn bases_aplicables(t: &TablaAnual, tramo: &Tramo, societario: bool) -> (Money, Money) {
    let min = if societario {
        tramo.base_min.max(t.base_min_societarios)
    } else {
        tramo.base_min
    };
    (min, tramo.base_max.max(min))
}

/// Sentido de la regularización.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Sentido {
    /// La base elegida está dentro del tramo de los rendimientos reales.
    Ninguna,
    /// Base inferior a la mínima del tramo real: la TGSS reclama la diferencia.
    AIngresar,
    /// Base superior a la máxima del tramo real: la TGSS devuelve la diferencia.
    ADevolver,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Regularizacion {
    pub tramo: Tramo,
    pub tramo_nombre: String,
    pub base_min: Money,
    pub base_max: Money,
    pub base_elegida: Money,
    /// Base por la que tendría que haber cotizado (la elegida, llevada dentro del tramo).
    pub base_correcta: Money,
    pub cuota_pagada: Money,
    pub cuota_correcta: Money,
    /// Importe de la regularización (siempre positivo; el sentido dice a quién).
    pub importe: Money,
    pub sentido: Sentido,
    pub meses: u8,
}

/// Estima la regularización con la misma base todos los meses de alta.
pub fn regularizacion(
    t: &TablaAnual,
    mensual: Money,
    base_elegida: Money,
    meses: u8,
    societario: bool,
) -> Regularizacion {
    let tramo = *tramo_de(t, mensual);
    let (base_min, base_max) = bases_aplicables(t, &tramo, societario);
    let base_correcta = base_elegida.clamp(base_min, base_max);
    let meses = meses.clamp(1, 12);
    let n = i64::from(meses);
    let mul = |m: Money| Money::from_cents(m.cents() * n);
    let cuota_pagada = mul(cuota(base_elegida, t.tipo));
    let cuota_correcta = mul(cuota(base_correcta, t.tipo));
    let (importe, sentido) = if base_elegida < base_min {
        (cuota_correcta - cuota_pagada, Sentido::AIngresar)
    } else if base_elegida > base_max {
        (cuota_pagada - cuota_correcta, Sentido::ADevolver)
    } else {
        (Money::ZERO, Sentido::Ninguna)
    };
    Regularizacion {
        tramo_nombre: tramo.nombre(),
        tramo,
        base_min,
        base_max,
        base_elegida,
        base_correcta,
        cuota_pagada,
        cuota_correcta,
        importe,
        sentido,
        meses,
    }
}

/// Plazo para pedir un cambio de base (o del rendimiento previsto) y desde cuándo vale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct VentanaCambio {
    pub desde: Date,
    pub hasta: Date,
    pub efectos: Date,
}

fn ymd(y: i32, m: Month, d: u8) -> Date {
    Date::from_calendar_date(y, m, d).unwrap_or(Date::MIN)
}

/// Las seis ventanas del año: pedido entre el 1 de enero y el último día de febrero, efectos el
/// 1 de marzo; marzo-abril → 1 de mayo; mayo-junio → 1 de julio; julio-agosto → 1 de
/// septiembre; septiembre-octubre → 1 de noviembre; noviembre-diciembre → 1 de enero del año
/// siguiente (art. 45.1 del RD 2064/1995, letras a a f, en la redacción del art. 5.1 del RDL
/// 13/2022; el antiguo art. 43 bis está derogado desde el 1-1-2023).
pub fn ventanas_cambio(year: i32) -> Vec<VentanaCambio> {
    use Month::*;
    [
        (January, February, March),
        (March, April, May),
        (May, June, July),
        (July, August, September),
        (September, October, November),
    ]
    .into_iter()
    .map(|(a, b, e)| VentanaCambio {
        desde: ymd(year, a, 1),
        hasta: ymd(year, b, b.length(year)),
        efectos: ymd(year, e, 1),
    })
    .chain(std::iter::once(VentanaCambio {
        desde: ymd(year, November, 1),
        hasta: ymd(year, December, 31),
        efectos: ymd(year + 1, January, 1),
    }))
    .collect()
}

/// La ventana abierta el día `hoy` (siempre hay una).
pub fn ventana_abierta(hoy: Date) -> VentanaCambio {
    ventanas_cambio(hoy.year())
        .into_iter()
        .find(|v| hoy >= v.desde && hoy <= v.hasta)
        .unwrap_or(VentanaCambio {
            desde: hoy,
            hasta: hoy,
            efectos: hoy,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn t2025() -> &'static TablaAnual {
        tabla(2025).unwrap()
    }

    #[test]
    fn tramos_contiguos_y_ordenados() {
        for t in &TABLAS {
            for w in t.tramos.windows(2) {
                assert_eq!(w[0].hasta, Some(w[1].desde), "{}", t.year);
                assert!(w[0].base_min <= w[0].base_max);
            }
            assert!(t.tramos.last().unwrap().hasta.is_none());
            // Sólo falta norma del último año (2027): 2025 y 2026 están cotejados con el BOE.
            assert_eq!(t.pendiente.is_empty(), t.year <= 2026, "{}", t.year);
        }
        // Años posteriores: la última tabla conocida; anteriores: ninguna.
        assert_eq!(tabla(2030).unwrap().year, 2027);
        // Hasta el último ejercicio previsto, cada año con la suya.
        for y in 2025..=crate::parametros::ULTIMO_EJERCICIO_PREVISTO {
            assert_eq!(tabla(y).unwrap().year, y);
        }
        assert!(tabla(2022).is_none());
    }

    #[test]
    fn tramo_por_rendimiento_mensual() {
        let t = t2025();
        assert_eq!(tramo_de(t, eur(-500, 0)).nombre(), "Reducida 1");
        assert_eq!(tramo_de(t, eur(670, 0)).nombre(), "Reducida 1");
        assert_eq!(tramo_de(t, eur(670, 1)).nombre(), "Reducida 2");
        assert_eq!(tramo_de(t, eur(1166, 70)).nombre(), "General 1");
        assert_eq!(tramo_de(t, eur(2500, 0)).nombre(), "General 7");
        assert_eq!(tramo_de(t, eur(9000, 0)).nombre(), "General 12");
    }

    #[test]
    fn rendimiento_del_societario_con_dividendos_y_3_por_ciento() {
        // 24.000 € de sueldo, 1.200 € de cuota pagada por la sociedad (especie), 6.000 € de
        // dividendos al 100 % y 500 € de gastos: 30.700 € − 3 % = 29.779 €; 2.481,58 €/mes.
        let r = rendimiento(&EntradaRendimiento {
            dinerarias: eur(24_000, 0),
            especie: eur(1_200, 0),
            gastos: eur(500, 0),
            dividendos: eur(6_000, 0),
            pct_dividendos: Rate::percent(100),
            otros: Money::ZERO,
            societario: true,
            meses: 12,
        });
        assert_eq!(r.neto, eur(30_700, 0));
        assert_eq!(r.deduccion, eur(921, 0));
        assert_eq!(r.computable, eur(29_779, 0));
        assert_eq!(r.mensual, eur(2_481, 58));
        assert_eq!(tramo_de(t2025(), r.mensual).nombre(), "General 7");
        // Sin dividendos y como autónomo no societario: 7 %.
        let r = rendimiento(&EntradaRendimiento {
            dinerarias: eur(12_000, 0),
            pct_dividendos: Rate::ZERO,
            dividendos: eur(6_000, 0),
            societario: false,
            meses: 6,
            ..Default::default()
        });
        assert_eq!(r.dividendos_computados, Money::ZERO);
        assert_eq!(r.computable, eur(11_160, 0));
        assert_eq!(r.mensual, eur(1_860, 0));
        // Pérdidas: sin deducción.
        let r = rendimiento(&EntradaRendimiento {
            gastos: eur(1_000, 0),
            meses: 12,
            ..Default::default()
        });
        assert_eq!(r.deduccion, Money::ZERO);
        assert_eq!(r.computable, eur(-1_000, 0));
    }

    #[test]
    fn regularizacion_a_ingresar_a_devolver_y_sin_ella() {
        let t = t2025();
        // General 7 (2.481,58 €/mes): base de 1.000 € < mínima 1.356,21 → a ingresar
        // (1.356,21 − 1.000) × 31,40 % × 12.
        let r = regularizacion(t, eur(2_481, 58), eur(1_000, 0), 12, true);
        assert_eq!(r.sentido, Sentido::AIngresar);
        assert_eq!(r.base_correcta, eur(1_356, 21));
        assert_eq!(r.cuota_pagada, eur(3_768, 0));
        assert_eq!(cuota(eur(1_356, 21), t.tipo), eur(425, 85));
        assert_eq!(r.importe, eur(5_110, 20) - eur(3_768, 0));
        // Base de 3.000 € > máxima 2.760 → a devolver (240 × 31,40 % × 12 = 904,32 €).
        let r = regularizacion(t, eur(2_481, 58), eur(3_000, 0), 12, true);
        assert_eq!(r.sentido, Sentido::ADevolver);
        assert_eq!(r.importe, eur(904, 32));
        // Dentro del tramo: nada.
        let r = regularizacion(t, eur(2_481, 58), eur(1_500, 0), 12, true);
        assert_eq!(r.sentido, Sentido::Ninguna);
        assert_eq!(r.importe, Money::ZERO);
    }

    #[test]
    fn el_societario_no_baja_de_su_base_minima() {
        let t = t2025();
        // Reducida 1: mínima 653,59 €, pero el societario cotiza por 1.000 € como poco.
        let r = regularizacion(t, eur(500, 0), eur(653, 59), 12, true);
        assert_eq!(r.base_min, eur(1_000, 0));
        assert_eq!(r.base_max, eur(1_000, 0));
        assert_eq!(r.sentido, Sentido::AIngresar);
        let r = regularizacion(t, eur(500, 0), eur(653, 59), 12, false);
        assert_eq!(r.sentido, Sentido::Ninguna);
    }

    #[test]
    fn en_2026_sube_la_base_maxima_y_la_minima_de_los_societarios() {
        // Orden PJC/297/2026: mismos tramos y bases mínimas que en 2025; base máxima de los
        // tramos 11 y 12, 5.101,20 €; societarios, la mínima del grupo 7 (1.424,40 €); MEI 0,90 %.
        let (a, b) = (tabla(2025).unwrap(), tabla(2026).unwrap());
        for (x, y) in a.tramos.iter().zip(b.tramos) {
            assert_eq!(
                (x.desde, x.hasta, x.base_min),
                (y.desde, y.hasta, y.base_min)
            );
        }
        assert_eq!(a.tramos[14].base_max, eur(4_909, 50));
        assert_eq!(b.tramos[13].base_max, eur(5_101, 20));
        assert_eq!(b.tramos[14].base_max, eur(5_101, 20));
        assert_eq!(b.tramos[12].base_max, eur(4_050, 0));
        assert_eq!((a.tipo.bp(), b.tipo.bp()), (3140, 3150));
        assert_eq!(a.base_min_societarios, eur(1_000, 0));
        assert_eq!(b.base_min_societarios, eur(1_424, 40));
        // General 7 en 2026: el societario no baja de 1.424,40 € aunque el tramo empiece en
        // 1.356,21 €.
        let r = regularizacion(b, eur(2_481, 58), eur(1_356, 21), 12, true);
        assert_eq!(r.base_min, eur(1_424, 40));
        assert_eq!(r.sentido, Sentido::AIngresar);
        let r = regularizacion(b, eur(2_481, 58), eur(1_356, 21), 12, false);
        assert_eq!(r.sentido, Sentido::Ninguna);
    }

    #[test]
    fn noventa_dias_de_alta_como_societario() {
        // Alta el 3 de octubre de 2026: 90 días hasta el 31 de diciembre (29 + 30 + 31). Cumple.
        let d = dias_alta_societario(2026, Some(date!(2026 - 10 - 03)), 3);
        assert_eq!(d, 90);
        assert!(cumple_dias_societario(d));
        // Un día después ya no: 89 días.
        let d = dias_alta_societario(2026, Some(date!(2026 - 10 - 04)), 3);
        assert_eq!(d, 89);
        assert!(!cumple_dias_societario(d));
        // Alta de años anteriores (o sin fecha): el año entero; 2028 es bisiesto.
        assert_eq!(
            dias_alta_societario(2026, Some(date!(2020 - 05 - 01)), 12),
            365
        );
        assert_eq!(dias_alta_societario(2028, None, 12), 366);
        // De antes, pero de baja a los dos meses: unos 60 días, no llega.
        assert_eq!(dias_alta_societario(2026, None, 2), 60);
        // Alta después del año: cero.
        assert_eq!(
            dias_alta_societario(2026, Some(date!(2027 - 01 - 10)), 12),
            0
        );
    }

    #[test]
    fn seis_ventanas_de_cambio_de_base() {
        let v = ventanas_cambio(2028);
        assert_eq!(v.len(), 6);
        assert_eq!(v[0].hasta, date!(2028 - 02 - 29));
        assert_eq!(v[0].efectos, date!(2028 - 03 - 01));
        assert_eq!(v[5].efectos, date!(2029 - 01 - 01));
        let a = ventana_abierta(date!(2026 - 11 - 16));
        assert_eq!(a.hasta, date!(2026 - 12 - 31));
        assert_eq!(a.efectos, date!(2027 - 01 - 01));
    }
}
