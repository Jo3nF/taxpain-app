//! Impuesto sobre Sociedades (Modelo 200): dividendos de participadas sin la exención del art.
//! 21 LIS, bases imponibles negativas compensadas de más, reserva de nivelación (con el acta que
//! aprueba las cuentas y el balance del 200 siguiente) y retenciones sobre dividendos, con una
//! estimación de lo que se pagó de más.
//!
//! La estimación rehace la liquidación del 200 presentado con sus propias casillas (base previa
//! [00550], reserva de capitalización [01032], bases negativas [00547], reserva de nivelación
//! [01034] y [01033], tipo según la cifra de negocios [00255]) y sólo da una cifra si el cálculo
//! con los datos originales reproduce la cuota íntegra presentada [00562]. Es orientativa.

use std::collections::BTreeMap;

use taxpain_core::calendar::{Model, Persona, TaxpayerProfile, calendar_for, deadlines_in_year};
use taxpain_core::money::{Money, Rate};
use taxpain_models::m200::{TaxRegime, gross_tax_for};
use time::{Date, Month};

use super::actas::Aplicacion;
use super::cruces::datos::MovBanco;
use super::expediente::{Expediente, Modelo};
use super::participadas::{Confirmado, bruto_con_retencion};
use super::{Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO, eur, fecha};

/// Casillas del 200 que se usan (sin ceros a la izquierda).
mod c {
    pub const RESULTADO: u32 = 500;
    pub const CIFRA_NEGOCIOS: u32 = 255;
    pub const DIVIDENDOS: u32 = 298;
    pub const DIVIDENDOS_GRUPO: u32 = 299;
    pub const BASE_PREVIA: u32 = 550;
    pub const BASE_TRAS_BIN: u32 = 552;
    pub const BIN_COMPENSADAS: u32 = 547;
    pub const CAPITALIZACION: u32 = 1032;
    pub const NIVELACION_ADICION: u32 = 1033;
    pub const NIVELACION: u32 = 1034;
    /// Reserva de nivelación del balance (patrimonio neto) al cierre del ejercicio. El mismo
    /// número sale después en la liquidación (gastos no deducibles del art. 15.a LIS), pero vale
    /// la primera vez que aparece: el balance va antes.
    pub const RESERVA_NIVELACION_BALANCE: u32 = 1002;
    /// Aplicación del resultado que declara el propio 200 (página 20): a la reserva de
    /// nivelación y el total aplicado.
    pub const APLICACION_NIVELACION: u32 = 1271;
    pub const APLICACION_TOTAL: u32 = 666;
    pub const BASE_IMPONIBLE: u32 = 1330;
    pub const CUOTA_INTEGRA: u32 = 562;
    /// Tipo de gravamen de la liquidación (página 14, «15,00» en las de nueva creación).
    pub const TIPO_GRAVAMEN: u32 = 558;
    /// Marcas de entidad de nueva creación al tipo reducido del 15 %: DT 22.ª LIS (entidades
    /// constituidas entre 2013 y 2014) [00063] y art. 29.1 LIS [00071]. En el fichero valen 1.
    pub const NUEVA_CREACION_DT22: u32 = 63;
    pub const NUEVA_CREACION: u32 = 71;
    pub const RESULTADO_LIQUIDACION: u32 = 1586;
    pub const A_INGRESAR_DEVOLVER: u32 = 621;
    pub const RETENCIONES_CAPITAL: u32 = 1785;
    /// Exención de dividendos de entidades residentes (art. 21.1, 21.10 y DT 40.ª LIS).
    pub const EXENCION_RESIDENTES: u32 = 370;
    pub const EXENCION_NO_RESIDENTES: u32 = 2181;
    /// Exención del art. 21.11 LIS (residentes y no residentes).
    pub const EXENCION_21_11_R: u32 = 1764;
    pub const EXENCION_21_11_NR: u32 = 1765;
    /// Cuadro de participaciones de la declarante (página 2, apartado B.1), totales: valor
    /// nominal, valor en libros y dividendos cobrados de las participadas en el ejercicio.
    pub const PARTICIPACIONES_NOMINAL: u32 = 1501;
    pub const PARTICIPACIONES_LIBROS: u32 = 1502;
    pub const DIVIDENDOS_PARTICIPADAS: u32 = 1503;
    /// Balance (página 3): inversiones en empresas del grupo y asociadas a largo plazo y, dentro,
    /// sus instrumentos de patrimonio (las participaciones). Los mismos números salen después en
    /// las deducciones por doble imposición, pero vale la primera vez: el balance va antes.
    pub const GRUPO_LARGO_PLAZO: u32 = 118;
    pub const GRUPO_PATRIMONIO: u32 = 119;
    pub const ACTIVO_NO_CORRIENTE: u32 = 101;
}

/// Régimen del tipo de gravamen según la cifra de negocios del ejercicio anterior (art. 29.1 y
/// 101 LIS), `anterior`: la [00255] del 200 del año antes si está en la carpeta; si no, la del
/// propio 200 como aproximación. Hasta 2022 no había tipo reducido para las de menos de 1 M€.
/// Una entidad de nueva creación (15 % en el primer periodo con base positiva y el siguiente,
/// art. 29.1 LIS, segundo párrafo) se reconoce por la marca del 200 ([00071], o [00063] para
/// las de la DT 22.ª) o por el tipo de gravamen de la liquidación [00558] al 15 %.
fn regimen(m: &Modelo, y: i32, anterior: Option<Money>) -> TaxRegime {
    let marcada = |n: u32| m.c(n) == Money::from_euros(1, 0);
    if marcada(c::NUEVA_CREACION)
        || marcada(c::NUEVA_CREACION_DT22)
        || m.c(c::TIPO_GRAVAMEN) == Money::from_euros(15, 0)
    {
        return TaxRegime::NuevaCreacion;
    }
    let cifra = anterior.unwrap_or_else(|| m.c(c::CIFRA_NEGOCIOS));
    if y <= 2022 || cifra >= Money::from_euros(10_000_000, 0) {
        TaxRegime::General
    } else if cifra < Money::from_euros(1_000_000, 0) {
        TaxRegime::Micro
    } else {
        TaxRegime::ReducidaDimension
    }
}

/// Liquidación rehecha: base imponible, cuota íntegra y bases negativas compensadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Calculo {
    base: Money,
    cuota: Money,
    bin: Money,
}

/// Rehace la liquidación del 200 quitando `menos` de la base previa (la exención que faltó) y,
/// si se da, con otras bases negativas compensadas (`bin`). `anterior`: cifra de negocios del
/// ejercicio anterior, que decide el tipo.
fn calcular(
    y: i32,
    m: &Modelo,
    anterior: Option<Money>,
    menos: Money,
    bin: Option<Money>,
) -> Calculo {
    let reg = regimen(m, y, anterior);
    let cero = Money::ZERO;
    let previa_orig = m.c(c::BASE_PREVIA);
    let previa = previa_orig - menos;
    // Reserva de capitalización: como mucho un porcentaje de la base previa (art. 25 LIS; desde
    // 2025, 20 % o 25 % en las de menos de 1 M€). Si estaba en el límite, baja con la base.
    let pct = if y >= 2025 {
        if reg == TaxRegime::Micro { 25 } else { 20 }
    } else {
        10
    };
    let tope = |b: Money| b.max(cero).apply_rate(Rate::percent(pct));
    let cap_orig = m.c(c::CAPITALIZACION);
    let cap = cap_orig.min(tope(previa));
    let bin_orig = m.c(c::BIN_COMPENSADAS);
    let bin = bin.unwrap_or(bin_orig).min((previa - cap).max(cero));
    let tras_bin = previa - cap - bin;
    // Reserva de nivelación: hasta el 10 % de la base y 1 M€, prorrateado si el periodo dura menos
    // de un año (art. 105.1 LIS, leído en el BOE el 2026-10-27: «la minoración no podrá superar el
    // importe de 1 millón de euros. Si el período impositivo tuviera una duración inferior a un
    // año, el importe de la minoración no podrá superar el resultado de multiplicar 1 millón de
    // euros por la proporción existente entre la duración del período impositivo respecto del
    // año»). Si era el 10 %, se rehace con la base nueva.
    let niv_orig = m.c(c::NIVELACION);
    let tras_bin_orig = previa_orig - cap_orig - bin_orig;
    let dias = m.dias_periodo();
    let diez = |b: Money| {
        b.max(cero)
            .apply_rate(Rate::percent(10))
            .min(tope_nivelacion(dias))
    };
    let niv = if niv_orig.is_zero() {
        cero
    } else if (niv_orig - diez(tras_bin_orig)).abs() <= UN_EURO {
        diez(tras_bin)
    } else {
        niv_orig.min(diez(tras_bin))
    };
    let base = tras_bin - niv + m.c(c::NIVELACION_ADICION);
    Calculo {
        base,
        cuota: gross_tax_for(base, reg, y, dias).0,
        bin,
    }
}

/// Tope de la minoración por reserva de nivelación: 1 M€ o, si el periodo impositivo dura
/// menos de un año, 1 M€ × días/365 (art. 105.1 LIS, segundo párrafo), al céntimo.
fn tope_nivelacion(dias: i64) -> Money {
    let millon = Money::from_euros(1_000_000, 0);
    if dias <= 0 || dias >= 365 {
        return millon;
    }
    let num = i128::from(millon.cents()) * i128::from(dias);
    let q = num / 365;
    let q = if 2 * (num % 365) >= 365 { q + 1 } else { q };
    Money::from_cents(i64::try_from(q).unwrap_or(millon.cents()))
}

/// La liquidación rehecha con los datos originales, si reproduce la cuota íntegra presentada
/// [00562] (con un euro de margen); si no, no se da ninguna cifra.
fn reproducida(y: i32, m: &Modelo, anterior: Option<Money>) -> Option<Calculo> {
    let original = calcular(y, m, anterior, Money::ZERO, None);
    let presentada = m.c(c::CUOTA_INTEGRA);
    if presentada.is_zero() || (original.cuota - presentada).abs() > UN_EURO {
        return None;
    }
    Some(original)
}

/// Lo que cambia la cuota con la corrección, si el cálculo reproduce el 200 presentado.
fn ahorro(
    y: i32,
    m: &Modelo,
    anterior: Option<Money>,
    menos: Money,
    bin: Option<Money>,
) -> Option<Money> {
    let original = reproducida(y, m, anterior)?;
    Some(original.cuota - calcular(y, m, anterior, menos, bin).cuota)
}

/// Impuesto que se ahorró con la parte de la reserva de nivelación que no se dotó (toda, con
/// `dotado` a cero): la cuota íntegra con la rebaja de lo dotado menos la presentada, si el
/// cálculo reproduce el 200.
fn ahorro_nivelacion(y: i32, m: &Modelo, anterior: Option<Money>, dotado: Money) -> Option<Money> {
    let original = reproducida(y, m, anterior)?;
    let mut sin = m.clone();
    if dotado.cents() > 0 {
        sin.casillas.insert(c::NIVELACION, dotado);
    } else {
        sin.casillas.remove(&c::NIVELACION);
    }
    Some(calcular(y, &sin, anterior, Money::ZERO, None).cuota - original.cuota)
        .filter(|a| a.cents() > 0)
}

/// Último día para que la junta apruebe las cuentas de `y`: los seis meses siguientes al cierre
/// (art. 164.1 LSC); el 30 de junio de `y + 1` si el ejercicio acaba el 31 de diciembre.
fn limite_junta(y: i32, m: &Modelo) -> Option<Date> {
    let cierre = match m.periodo_hasta {
        Some(d) => d,
        None => Date::from_calendar_date(y, Month::December, 31).ok()?,
    };
    Some(crate::cierre::after_close(cierre, 6))
}

/// El último día de un plazo y si es exacto: sin los festivos del año en que acaba (TaxPain no
/// los tiene todos), es el último día sin trasladar, aproximado; el de verdad puede ser algún día
/// después, nunca antes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dia {
    pub dia: Date,
    pub exacto: bool,
}

impl Dia {
    /// «27/07/2030» o, si es aproximado, «25/07/2028 aprox.».
    pub fn texto(self) -> String {
        if self.exacto {
            fecha(self.dia)
        } else {
            format!("{} aprox.", fecha(self.dia))
        }
    }

    /// `aaaa-mm-dd`.
    pub fn iso(self) -> String {
        format!(
            "{:04}-{:02}-{:02}",
            self.dia.year(),
            u8::from(self.dia.month()),
            self.dia.day()
        )
    }
}

/// Los plazos del 200 con el calendario fiscal de la empresa: el cierre de su ejercicio, su
/// comunidad autónoma y sus festivos locales (los del perfil fiscal; sin perfil, los de una
/// sociedad limitada).
#[derive(Clone, Debug)]
pub(crate) struct Plazos200 {
    perfil: TaxpayerProfile,
}

impl Default for Plazos200 {
    fn default() -> Self {
        Plazos200 {
            perfil: TaxpayerProfile::sociedad_limitada(),
        }
    }
}

impl Plazos200 {
    pub fn de(exp: &Expediente) -> Self {
        let mut perfil = super::cruces::perfil_amplio(exp);
        // Si hay un 200, es una sociedad, diga lo que diga el perfil.
        perfil.persona = Persona::Juridica;
        Plazos200 { perfil }
    }

    /// Último día para presentar el 200 del ejercicio `y`: los 25 días naturales siguientes a los
    /// seis meses posteriores al cierre (art. 124.1 LIS; el 25 de julio de `y + 1` con cierre el
    /// 31 de diciembre), trasladado al siguiente día hábil si cae en sábado, domingo o festivo
    /// (art. 30.5 Ley 39/2015). Sin los festivos de ese año, el último día sin trasladar,
    /// aproximado.
    pub fn fin(&self, y: i32) -> Option<Dia> {
        for año in [y + 1, y + 2] {
            let (h, cargados) = calendar_for(año, self.perfil.ccaa, &self.perfil.festivos_locales);
            if let Some(d) = deadlines_in_year(&self.perfil, año, &h)
                .into_iter()
                .find(|d| d.model == Model::M200 && d.fiscal_year == y)
            {
                return Some(if cargados {
                    Dia {
                        dia: d.due_on,
                        exacto: true,
                    }
                } else {
                    Dia {
                        dia: d.nominal_end,
                        exacto: false,
                    }
                });
            }
        }
        Date::from_calendar_date(y + 1, Month::July, 25)
            .ok()
            .map(|dia| Dia { dia, exacto: false })
    }

    /// Hasta cuándo se puede pedir la rectificación del 200 del ejercicio `y` (y hasta cuándo
    /// puede Hacienda liquidarlo): cuatro años desde el día siguiente al fin del plazo de
    /// presentación (arts. 66 y 67.1 LGT), de fecha a fecha (art. 5.1 del Código Civil).
    pub fn rectificable(&self, y: i32) -> Option<Dia> {
        let f = self.fin(y)?;
        let (año, mes, dia) = (f.dia.year() + 4, f.dia.month(), f.dia.day());
        let dia = Date::from_calendar_date(año, mes, dia)
            .or_else(|_| Date::from_calendar_date(año, mes, dia - 1))
            .ok()?;
        Some(Dia {
            dia,
            exacto: f.exacto,
        })
    }
}

/// Cauce para corregir a favor del contribuyente el 200 del ejercicio `y`: desde los periodos
/// iniciados en 2024, la autoliquidación rectificativa del propio modelo (art. 120.4 LGT, art. 59
/// bis RIS y Orden HAC/657/2025), que sustituye a la solicitud de rectificación; para los
/// anteriores sigue la solicitud de rectificación (art. 120.3 LGT).
fn cauce(y: i32) -> &'static str {
    if y >= 2024 {
        "autoliquidación rectificativa del 200 (art. 120.4 LGT y art. 59 bis RIS)"
    } else {
        "solicitud de rectificación (art. 120.3 LGT)"
    }
}

/// Un 200 por ejercicio: el de la carpeta con más casillas o, si no hay, el guardado en TaxPain.
fn por_ejercicio(exp: &Expediente) -> BTreeMap<i32, (Modelo, String)> {
    let mut out: BTreeMap<i32, (Modelo, String)> = BTreeMap::new();
    for f in &exp.presentados {
        if f.model == "200" {
            out.insert(
                f.year,
                (
                    Modelo {
                        modelo: "200".into(),
                        ejercicio: f.year,
                        periodo: f.period.clone(),
                        casillas: f.boxes.iter().map(|(k, v)| (u32::from(*k), *v)).collect(),
                        ..Modelo::default()
                    },
                    f.filename.clone(),
                ),
            );
        }
    }
    let mut de_carpeta: BTreeMap<i32, (Modelo, String)> = BTreeMap::new();
    for (d, m) in exp.modelos() {
        // Un justificante cuyas casillas no se han podido leer no dice nada del impuesto.
        if m.modelo != "200" || m.sin_casillas {
            continue;
        }
        let mejor = de_carpeta
            .get(&m.ejercicio)
            .is_none_or(|(o, _)| m.casillas.len() > o.casillas.len());
        if mejor {
            de_carpeta.insert(m.ejercicio, (m.clone(), d.ruta.clone()));
        }
    }
    out.extend(de_carpeta);
    out
}

pub(crate) fn revisar(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let doscientos = por_ejercicio(exp);
    let plazos = Plazos200::de(exp);
    dividendos_cobrados(exp, &doscientos, &plazos, hoy, s);
    if doscientos.is_empty() {
        return;
    }
    tabla(&doscientos, s);
    let con_exencion: Vec<i32> = doscientos
        .iter()
        .filter(|(_, (m, _))| exencion_aplicada(m).cents() > 0)
        .map(|(y, _)| *y)
        .collect();
    // Cifra de negocios del ejercicio anterior, que decide el tipo (art. 29.1 LIS).
    let anterior = |y: i32| {
        doscientos
            .get(&(y - 1))
            .map(|(m, _)| m.c(c::CIFRA_NEGOCIOS))
    };
    let sumadas = nivelacion_sumada(&doscientos);
    for (y, (m, fichero)) in &doscientos {
        let rectificar = exencion(*y, m, anterior(*y), fichero, &con_exencion, &plazos, hoy, s);
        participadas(*y, m, fichero, rectificar, s);
        participaciones_del_balance(*y, m, fichero, &plazos, hoy, s);
        let (acta, dotan) = actas_de(exp, *y);
        let dotacion = Dotacion {
            siguiente: doscientos.get(&(y + 1)).map(|(m, f)| (m, f.as_str())),
            acta,
            dotan,
            sumada_en: sumadas.get(y).copied(),
        };
        nivelacion(*y, m, &dotacion, anterior(*y), fichero, &plazos, hoy, s);
        retenciones(*y, m, fichero, s);
    }
    bases_negativas(&doscientos, s);
    orden_rectificativas(&doscientos, &plazos, hoy, s);
}

/// Las actas de la carpeta del ejercicio `y` (las de después de su cierre): la que aprueba sus
/// cuentas (la primera, si hay varias) y las que llevan algo a la reserva de nivelación (esa misma
/// o una decisión posterior), una por fecha: dos con la misma fecha son copias de la misma acta.
#[allow(clippy::type_complexity)]
fn actas_de(exp: &Expediente, y: i32) -> (Option<(&Aplicacion, &str)>, Vec<(&Aplicacion, &str)>) {
    let del_año: Vec<(&Aplicacion, &str)> = exp
        .docs
        .iter()
        .filter_map(|d| {
            d.aplicacion
                .as_ref()
                .filter(|a| a.del_ejercicio(y))
                .map(|a| (a, d.ruta.as_str()))
        })
        .collect();
    let aprobacion = del_año
        .iter()
        .filter(|(a, _)| a.aprueba)
        .min_by_key(|(a, ruta)| (a.fecha.is_none(), a.fecha, *ruta))
        .copied();
    let mut dotan: BTreeMap<Option<Date>, (&Aplicacion, &str)> = BTreeMap::new();
    for (a, ruta) in del_año {
        if a.nivelacion().cents() <= 0 {
            continue;
        }
        let e = dotan.entry(a.fecha).or_insert((a, ruta));
        if a.nivelacion() > e.0.nivelacion() {
            *e = (a, ruta);
        }
    }
    (aprobacion, dotan.into_values().collect())
}

/// En qué 200 se acabó de sumar a la base cada minoración por reserva de nivelación ([01034]),
/// por ejercicio: las adiciones [01033] de los cinco años siguientes, empezando por la
/// minoración más antigua.
fn nivelacion_sumada(doscientos: &BTreeMap<i32, (Modelo, String)>) -> BTreeMap<i32, i32> {
    let mut pendientes: Vec<(i32, Money)> = Vec::new();
    let mut out = BTreeMap::new();
    for (z, (m, _)) in doscientos {
        let mut adicion = m.c(c::NIVELACION_ADICION);
        for (y, queda) in &mut pendientes {
            if adicion.cents() <= 0 {
                break;
            }
            if *y >= *z || *z > *y + 5 {
                continue;
            }
            let toma = adicion.min(*queda);
            *queda -= toma;
            adicion -= toma;
            if *queda <= UN_EURO {
                out.insert(*y, *z);
            }
        }
        pendientes.retain(|(_, queda)| *queda > UN_EURO);
        let niv = m.c(c::NIVELACION);
        if niv.cents() > 0 {
            pendientes.push((*z, niv));
        }
    }
    out
}

fn exencion_aplicada(m: &Modelo) -> Money {
    m.c(c::EXENCION_RESIDENTES)
        + m.c(c::EXENCION_NO_RESIDENTES)
        + m.c(c::EXENCION_21_11_R)
        + m.c(c::EXENCION_21_11_NR)
}

fn base_final(m: &Modelo) -> Money {
    if m.casillas.contains_key(&c::BASE_IMPONIBLE) {
        m.c(c::BASE_IMPONIBLE)
    } else {
        m.c(c::BASE_TRAS_BIN)
    }
}

fn tabla(doscientos: &BTreeMap<i32, (Modelo, String)>, s: &mut Salida) {
    let filas = doscientos
        .iter()
        .map(|(y, (m, _))| {
            // El resultado: sin leer si no está o si no cuadró al leer el justificante (no es
            // cero).
            let resultado = [c::RESULTADO_LIQUIDACION, c::A_INGRESAR_DEVOLVER]
                .iter()
                .find(|k| !m.dudosas.contains(k) && m.casillas.contains_key(k))
                .map_or_else(|| "sin leer".to_owned(), |k| eur(m.c(*k)));
            vec![
                y.to_string(),
                eur(m.c(c::RESULTADO)),
                eur(m.c(c::DIVIDENDOS)),
                eur(exencion_aplicada(m)),
                eur(m.c(c::BIN_COMPENSADAS)),
                eur(m.c(c::NIVELACION)),
                eur(base_final(m)),
                eur(m.c(c::CUOTA_INTEGRA)),
                resultado,
            ]
        })
        .collect();
    s.tabla(
        "Impuesto sobre Sociedades por ejercicio",
        "Impuesto sobre Sociedades",
        &[
            "Ejercicio",
            "Resultado contable [00500]",
            "Dividendos de participadas [00298]",
            "Exención aplicada [00370]",
            "Bases negativas compensadas [00547]",
            "Reserva de nivelación [01034]",
            "Base imponible",
            "Cuota íntegra [00562]",
            "Resultado (+ a ingresar, − a devolver)",
        ],
        filas,
    );
}

/// La exención del art. 21 LIS que falta en el 200: el 95 % de los dividendos de participadas
/// (los del grupo, si los separa), si los hay y la exención está vacía. El 5 % restante tributa
/// como gastos de gestión (art. 21.10 LIS).
fn exento_que_falta(m: &Modelo) -> Option<Money> {
    let dividendos = m.c(c::DIVIDENDOS);
    let grupo = m.c(c::DIVIDENDOS_GRUPO);
    if dividendos.cents() <= 0 || exencion_aplicada(m).cents() > 0 {
        return None;
    }
    let base = if grupo.cents() > 0 { grupo } else { dividendos };
    Some(base.apply_rate(Rate::percent(95)))
}

/// Las bases negativas que deja para los años siguientes rectificar el 200 `m` con la exención
/// que falta (estimación): lo que su base queda en negativo y lo que ya no hace falta compensar
/// de las de antes ([00547]). Sin la base previa [00550], con la base final. Cero si no falta la
/// exención o no se sabe la base.
fn bases_que_deja(y: i32, m: &Modelo, anterior: Option<Money>) -> Money {
    let cero = Money::ZERO;
    let Some(exento) = exento_que_falta(m) else {
        return cero;
    };
    if m.casillas.contains_key(&c::BASE_PREVIA) {
        let r = calcular(y, m, anterior, exento, None);
        (-r.base).max(cero) + (m.c(c::BIN_COMPENSADAS) - r.bin).max(cero)
    } else if m.casillas.contains_key(&c::BASE_IMPONIBLE)
        || m.casillas.contains_key(&c::BASE_TRAS_BIN)
    {
        (exento - base_final(m).max(cero)).max(cero)
    } else {
        cero
    }
}

/// Dividendos de participadas que tributaron enteros: sin la exención del art. 21 LIS. Devuelve
/// si hay que rectificar el 200 (aún se puede).
#[allow(clippy::too_many_arguments)]
fn exencion(
    y: i32,
    m: &Modelo,
    anterior: Option<Money>,
    fichero: &str,
    con_exencion: &[i32],
    plazos: &Plazos200,
    hoy: Date,
    s: &mut Salida,
) -> bool {
    let Some(exento) = exento_que_falta(m) else {
        return false;
    };
    let dividendos = m.c(c::DIVIDENDOS);
    let grupo = m.c(c::DIVIDENDOS_GRUPO);
    let importe = ahorro(y, m, anterior, exento, None).filter(|a| a.cents() > 0);
    let hasta = plazos.rectificable(y);
    let caducado = hasta.is_some_and(|h| hoy > h.dia);
    let mut detalle = format!(
        "En el 200 de {y} hay {} de dividendos de participaciones{} y la exención (casilla \
         [00370]) está vacía: tributaron enteros. Con al menos el 5 % de la participada durante \
         un año, el 95 % está exento (art. 21.1 y 21.10 LIS): {} que no debían tributar.",
        eur(dividendos),
        if grupo.cents() > 0 {
            " en empresas del grupo y asociadas"
        } else {
            ""
        },
        eur(exento)
    );
    let otros: Vec<String> = con_exencion
        .iter()
        .filter(|o| **o != y)
        .map(i32::to_string)
        .collect();
    if !otros.is_empty() {
        detalle.push_str(&format!(
            " En el 200 de {} sí se aplicó.",
            otros.join(" y de ")
        ));
    }
    match importe {
        Some(a) => detalle.push_str(&format!(
            " Rehecha la liquidación con la exención, la cuota baja unos {} (estimación).",
            eur(a)
        )),
        None if base_final(m).cents() <= 0 => detalle.push_str(
            " Ese año la base salió negativa: la exención no devuelve dinero, pero aumenta las \
             bases negativas que se pueden compensar después.",
        ),
        None => {}
    }
    if let Some(h) = hasta {
        detalle.push_str(&if caducado {
            format!(
                " El plazo para pedir la rectificación acabó el {}.",
                h.texto()
            )
        } else {
            format!(
                " Hasta el {} (art. 66 LGT) se puede presentar una {}.",
                h.texto(),
                cauce(y)
            )
        });
    }
    let gravedad = if caducado {
        Gravedad::Info
    } else if grupo.cents() > 0 {
        Gravedad::Error
    } else {
        Gravedad::Aviso
    };
    s.add(
        Nuevo {
            codigo: "sociedades.exencion_dividendos",
            area: Area::Sociedades,
            gravedad,
            responsable: Responsable::Gestoria,
            titulo: format!(
                "Dividendos de {y} sin la exención del art. 21 LIS{}",
                if grupo.cents() > 0 {
                    ""
                } else {
                    " (comprueba el porcentaje)"
                }
            ),
            ficheros: vec![fichero.to_owned()],
        }
        .detalle(detalle),
        Some(y),
        if caducado { None } else { importe },
    );
    if !caducado {
        let mut que = match importe {
            Some(a) => format!(
                "Rectificar el Impuesto sobre Sociedades de {y}, con una {}, para aplicar la \
                 exención del art. 21 LIS a los dividendos de las participadas (unos {} a \
                 devolver)",
                cauce(y),
                eur(a)
            ),
            None => format!(
                "Rectificar el Impuesto sobre Sociedades de {y}, con una {}, para aplicar la \
                 exención del art. 21 LIS a los dividendos de las participadas",
                cauce(y)
            ),
        };
        if dividendos_sin_detallar(m).is_some() {
            que.push_str(
                ", y rellenar en el cuadro de participaciones lo cobrado de cada una (casilla \
                 01503)",
            );
        }
        s.pedir(Destinatario::Gestoria, que);
        s.pedir(
            Destinatario::Empresa,
            "Las escrituras o el libro de socios de las participadas (porcentaje y fecha de \
             entrada) y los certificados de los dividendos cobrados, para la rectificación",
        );
    }
    !caducado
}

/// Dividendos de participadas que el cuadro de participaciones (página 2, apartado B.1) no
/// recoge: el cuadro está relleno ([01501] o [01502]) y la casilla [01503] vacía. Sólo los que
/// tienen que estar ahí: los del grupo y asociadas, o los que se declararon exentos. Si el cuadro
/// no se ha leído, no se sabe y no se avisa.
fn dividendos_sin_detallar(m: &Modelo) -> Option<Money> {
    let cuadro = m.casillas.contains_key(&c::PARTICIPACIONES_NOMINAL)
        || m.casillas.contains_key(&c::PARTICIPACIONES_LIBROS);
    if !cuadro || m.c(c::DIVIDENDOS_PARTICIPADAS).cents() > 0 {
        return None;
    }
    let grupo = m.c(c::DIVIDENDOS_GRUPO);
    if grupo.cents() > 0 {
        return Some(grupo);
    }
    let dividendos = m.c(c::DIVIDENDOS);
    (exencion_aplicada(m).cents() > 0 && dividendos.cents() > 0).then_some(dividendos)
}

/// El cuadro de participaciones sin lo cobrado de cada participada (casilla [01503] vacía). No
/// cambia la cuota, pero es con lo que Hacienda contrasta la exención: en una rectificativa que
/// la pide, hay que rellenarlo.
fn participadas(y: i32, m: &Modelo, fichero: &str, rectificar: bool, s: &mut Salida) {
    let Some(dividendos) = dividendos_sin_detallar(m) else {
        return;
    };
    let mut detalle = format!(
        "El 200 de {y} tiene {} de dividendos de participadas, pero el cuadro de participaciones \
         (página 2, apartado B.1) no recoge lo cobrado de cada una: la casilla [01503] está \
         vacía. Por cada participada va lo que pagó ese año (según su certificado), y la [01503] \
         es la suma. No cambia la cuota, pero es con lo que Hacienda contrasta la exención del \
         art. 21 LIS.",
        eur(dividendos)
    );
    detalle.push_str(if rectificar {
        " Hay que rellenarlo en la rectificativa: si no, es fácil que pidan explicaciones y que \
         la devolución tarde más."
    } else {
        " Por sí solo no merece una rectificativa; si se rectifica este 200 por otra cosa, \
         conviene rellenarlo."
    });
    s.add(
        Nuevo {
            codigo: "sociedades.dividendos_sin_detallar",
            area: Area::Sociedades,
            gravedad: if rectificar {
                Gravedad::Aviso
            } else {
                Gravedad::Info
            },
            responsable: Responsable::Gestoria,
            titulo: format!("Dividendos de {y} sin detallar por participada (casilla 01503)"),
            ficheros: vec![fichero.to_owned()],
        }
        .detalle(detalle),
        Some(y),
        None,
    );
}

/// Las participaciones en empresas del grupo y asociadas del balance ([00119]) frente al valor en
/// libros del cuadro de participaciones (página 2, apartado B.1, [01502]): si el balance tiene
/// más, hay algo que no es una participación o falta una participada en el cuadro. Sólo con las
/// dos casillas leídas y el balance coherente ([00119] dentro de [00118] y esta dentro del activo
/// no corriente [00101]): con los mismos números hay casillas de otras páginas del 200. Que el
/// cuadro tenga más no se avisa: también recoge participaciones que no son del grupo.
fn participaciones_del_balance(
    y: i32,
    m: &Modelo,
    fichero: &str,
    plazos: &Plazos200,
    hoy: Date,
    s: &mut Salida,
) {
    let casilla = |n: u32| m.casillas.get(&n).copied();
    let (Some(balance), Some(total), Some(cuadro)) = (
        casilla(c::GRUPO_PATRIMONIO),
        casilla(c::GRUPO_LARGO_PLAZO),
        casilla(c::PARTICIPACIONES_LIBROS),
    ) else {
        return;
    };
    let coherente = balance <= total + UN_EURO
        && casilla(c::ACTIVO_NO_CORRIENTE).is_none_or(|a| total <= a + UN_EURO);
    let diferencia = balance - cuadro;
    if !coherente || diferencia <= UN_EURO {
        return;
    }
    let caducado = plazos.rectificable(y).is_some_and(|h| hoy > h.dia);
    let mut detalle = format!(
        "En el balance del 200 de {y}, los instrumentos de patrimonio de empresas del grupo y \
         asociadas a largo plazo (casilla [00119]) suman {}, pero el cuadro de participaciones \
         (página 2, apartado B.1) sólo recoge {} de valor en libros (casilla [01502]): {} de \
         diferencia. O hay algo que no es una participación (por ejemplo, un préstamo a un socio \
         contabilizado como participación: con un socio del 25 % o más es una operación \
         vinculada y tiene que llevar intereses a precio de mercado, art. 18.1 y 18.2.a LIS), o \
         falta una participada en el cuadro.",
        eur(balance),
        eur(cuadro),
        eur(diferencia)
    );
    detalle.push_str(if caducado {
        " Ese 200 ya no se puede rectificar, pero si la partida sigue en el balance, conviene \
         corregirla en el próximo."
    } else {
        " Qué hacer: pide a la gestoría el detalle de esa partida; si hay que corregirlo, en la \
         rectificativa de este 200 o en el próximo."
    });
    s.add(
        Nuevo {
            codigo: "sociedades.participaciones_balance",
            area: Area::Sociedades,
            gravedad: if caducado {
                Gravedad::Info
            } else {
                Gravedad::Aviso
            },
            responsable: Responsable::Gestoria,
            titulo: format!(
                "Participaciones del grupo en el 200 de {y}: el balance tiene {} más que el cuadro",
                eur(diferencia)
            ),
            ficheros: vec![fichero.to_owned()],
        }
        .detalle(detalle),
        Some(y),
        None,
    );
    if !caducado {
        s.pedir(
            Destinatario::Gestoria,
            format!(
                "Explicar los {} de participaciones del grupo del balance del 200 de {y} que no \
                 están en el cuadro de participaciones (casilla 01502)",
                eur(diferencia)
            ),
        );
    }
}

/// Lo que dice la carpeta de la dotación de la reserva de nivelación de un ejercicio.
#[derive(Debug, Default)]
struct Dotacion<'a> {
    /// El 200 del ejercicio siguiente, con la reserva de nivelación del balance [01002].
    siguiente: Option<(&'a Modelo, &'a str)>,
    /// El acta que aprueba las cuentas del ejercicio, con la aplicación del resultado.
    acta: Option<(&'a Aplicacion, &'a str)>,
    /// Las actas del ejercicio que llevan algo a la reserva de nivelación (la que aprueba las
    /// cuentas o una decisión posterior), una por fecha.
    dotan: Vec<(&'a Aplicacion, &'a str)>,
    /// Ejercicio del 200 en el que se acabó de sumar la minoración a la base ([01033]).
    sumada_en: Option<i32>,
}

/// De dónde se sabe que se dotó la reserva de nivelación.
enum Dotada<'a> {
    /// El balance del 200 siguiente: la reserva de nivelación [01002] sube al menos lo minorado.
    Balance {
        antes: Money,
        despues: Money,
        fichero: &'a str,
    },
    /// Las actas del ejercicio la dotan (la que aprueba las cuentas, una decisión posterior o
    /// entre las dos).
    Actas {
        dotado: Money,
        actas: Vec<(&'a Aplicacion, &'a str)>,
    },
    /// La aplicación del resultado que declara el propio 200 la dota ([01271]).
    Doscientos { dotado: Money },
}

impl<'a> Dotacion<'a> {
    /// Con el acta que aprueba las cuentas, que también cuenta como dotación si lleva algo a la
    /// reserva de nivelación.
    #[cfg(test)]
    fn con_acta(a: &'a Aplicacion, fichero: &'a str) -> Self {
        Dotacion {
            acta: Some((a, fichero)),
            dotan: if a.nivelacion().cents() > 0 {
                vec![(a, fichero)]
            } else {
                Vec::new()
            },
            ..Dotacion::default()
        }
    }

    /// Lo que llevan a la reserva de nivelación las actas del ejercicio.
    fn de_las_actas(&self) -> Money {
        self.dotan
            .iter()
            .fold(Money::ZERO, |s, (a, _)| s + a.nivelacion())
    }

    /// El acta que aprueba las cuentas, si se ha leído entera.
    fn acta_entera(&self) -> Option<(&'a Aplicacion, &'a str)> {
        self.acta.filter(|(a, _)| a.completa)
    }

    fn dotada(&self, m: &Modelo, niv: Money) -> Option<Dotada<'a>> {
        let balance = self.siguiente.and_then(|(sig, fichero)| {
            let antes = m.c(c::RESERVA_NIVELACION_BALANCE);
            let despues = sig.c(c::RESERVA_NIVELACION_BALANCE);
            (sig.casillas.contains_key(&c::RESERVA_NIVELACION_BALANCE)
                && despues - antes + UN_EURO >= niv)
                .then_some(Dotada::Balance {
                    antes,
                    despues,
                    fichero,
                })
        });
        let de_las_actas = self.de_las_actas();
        balance
            .or_else(|| {
                (!self.dotan.is_empty() && de_las_actas + UN_EURO >= niv).then(|| Dotada::Actas {
                    dotado: de_las_actas,
                    actas: self.dotan.clone(),
                })
            })
            .or_else(|| {
                // La del 200 no vale si el acta que aprueba las cuentas, leída entera, dice otra
                // cosa: entonces no se sabe cuál es la buena.
                let dotado = m.c(c::APLICACION_NIVELACION);
                (self.acta_entera().is_none() && dotado + UN_EURO >= niv)
                    .then_some(Dotada::Doscientos { dotado })
            })
    }
}

/// « del 30/06/2026» (o nada, sin fecha).
fn del(a: &Aplicacion) -> String {
    a.fecha
        .map(|f| format!(" del {}", fecha(f)))
        .unwrap_or_default()
}

/// « del 16/10/2026 y del 05/12/2026»: las partes juntas, con «y» antes de la última.
fn y_lista(partes: &[String]) -> String {
    match partes {
        [] => String::new(),
        [una] => una.clone(),
        [resto @ .., ultima] => format!("{} y{ultima}", resto.join(",")),
    }
}

/// Reserva de nivelación aplicada: la rebaja de la base se devuelve en los cinco años siguientes
/// (art. 105.2 LIS) y obliga a dotar una reserva indisponible con el resultado del ejercicio (art.
/// 105.3 LIS). Si no se dota, se paga el impuesto ahorrado más un 5 % y los intereses de demora
/// (art. 105.6 LIS). La contabilidad de TaxPain no tiene una cuenta para esta reserva: se sabe si
/// se dotó por el acta que aprueba las cuentas (la aplicación del resultado) o por el balance del
/// 200 siguiente; sin ninguno de los dos, antes del plazo de la junta está pendiente y, después,
/// hay que mirar el acta.
#[allow(clippy::too_many_arguments)]
fn nivelacion(
    y: i32,
    m: &Modelo,
    dotacion: &Dotacion<'_>,
    anterior: Option<Money>,
    fichero: &str,
    plazos: &Plazos200,
    hoy: Date,
    s: &mut Salida,
) {
    let niv = m.c(c::NIVELACION);
    if niv.cents() <= 0 {
        return;
    }
    let tope = tope_nivelacion(m.dias_periodo());
    if niv > tope + UN_EURO {
        s.add(
            Nuevo {
                codigo: "sociedades.reserva_nivelacion_tope",
                area: Area::Sociedades,
                gravedad: Gravedad::Error,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "Reserva de nivelación de {y} por encima del tope: {}",
                    eur(niv)
                ),
                ficheros: vec![fichero.to_owned()],
            }
            .detalle(format!(
                "La minoración por reserva de nivelación no puede pasar de {} ({} días de periodo \
                 impositivo: 1 M€ prorrateado, art. 105.1 LIS) y el 200 de {y} aplica {}.",
                eur(tope),
                m.dias_periodo(),
                eur(niv)
            )),
            Some(y),
            None,
        );
    }
    let ahorrado = ahorro_nivelacion(y, m, anterior, Money::ZERO);
    // Lo que se pagaría si no se dota: el impuesto ahorrado más un 5 % (sin los intereses).
    let riesgo = ahorrado.map(|a| a.apply_rate(Rate::percent(105)));
    let limite = limite_junta(y, m);
    let pendiente = limite.is_none_or(|l| hoy <= l);
    // Si la junta no la dotó, el incumplimiento es del ejercicio siguiente (el de la junta) y
    // prescribe con el plazo de su 200 (art. 66 LGT). `TODO(VERIFY)`: si se dota con beneficios de
    // años posteriores o se reparte antes de tiempo, el incumplimiento es de ese otro año.
    let prescrito = plazos.rectificable(y + 1).filter(|p| hoy > p.dia);
    if let Some(p) = prescrito {
        s.add(
            Nuevo {
                codigo: "sociedades.reserva_nivelacion",
                area: Area::Sociedades,
                gravedad: Gravedad::Info,
                responsable: Responsable::Gestoria,
                titulo: format!("Reserva de nivelación de {y}: {}", eur(niv)),
                ficheros: vec![fichero.to_owned()],
            }
            .detalle(format!(
                "El 200 de {y} rebajó la base en {} por la reserva de nivelación (art. 105 LIS): \
                 un aplazamiento que vuelve a la base en los cinco años siguientes, como tarde en \
                 {}. Si la junta no dotó la reserva, Hacienda ya no lo puede reclamar: el plazo \
                 acabó el {} (cuatro años desde el fin del plazo del 200 de {}, art. 66 LGT). \
                 Salvo que la reserva se dotara o se repartiera en un año posterior: entonces el \
                 plazo cuenta desde ese año.",
                eur(niv),
                y + 5,
                p.texto(),
                y + 1
            )),
            Some(y),
            None,
        );
        return;
    }
    if let Some(d) = dotacion.dotada(m, niv) {
        nivelacion_dotada(y, niv, &d, dotacion, fichero, s);
        return;
    }
    let del_200 = m.c(c::APLICACION_NIVELACION);
    if let Some((a, acta)) = dotacion.acta_entera()
        && del_200 + UN_EURO < niv
    {
        nivelacion_sin_dotar(y, m, anterior, niv, dotacion, a, acta, fichero, s);
        return;
    }
    let mut titulo = if pendiente {
        format!(
            "Reserva de nivelación de {y}: la junta tiene que dotar {}",
            eur(niv)
        )
    } else {
        format!(
            "Reserva de nivelación de {y}: comprueba que la junta dotó {}",
            eur(niv)
        )
    };
    if let Some(r) = riesgo {
        titulo.push_str(&format!(" (si no, unos {} más intereses)", eur(r)));
    }
    let mut detalle = format!(
        "El 200 de {y} rebaja la base en {} por la reserva de nivelación. No es un ahorro \
         definitivo, es un aplazamiento: esa rebaja se suma a la base de los cinco años \
         siguientes si alguno sale con base negativa (hasta ese importe) y, lo que quede, a la \
         de {} (art. 105.2 LIS). A cambio, la junta que aprueba las cuentas de {y} tiene que \
         dotar una reserva por ese importe con el beneficio de {y} (si no llega, con los primeros \
         beneficios de los años siguientes), que no se puede repartir mientras tanto (art. 105.3 \
         LIS).",
        eur(niv),
        y + 5
    );
    let mut ficheros = vec![fichero.to_owned()];
    match dotacion.acta {
        // Leída entera y sin la reserva, pero el 200 dice que se dotó: no se sabe cuál vale.
        Some((a, acta)) if a.completa => {
            let reparto = a.texto();
            detalle.push_str(&format!(
                " El acta{} que aprueba las cuentas de {y} no la dota{}, pero la aplicación del \
                 resultado que declara el propio 200 lleva {} a la reserva de nivelación (casilla \
                 [01271]): comprueba cuál de los dos es el bueno.",
                del(a),
                if reparto.is_empty() {
                    String::new()
                } else {
                    format!(" (reparte {reparto})")
                },
                eur(del_200)
            ));
            ficheros.push(acta.to_owned());
        }
        Some((a, acta)) => {
            detalle.push_str(&format!(
                " El acta{} que aprueba las cuentas de {y} no se ha podido leer entera (lleva \
                 partidas que TaxPain no reconoce, importes en columnas sin saber cuál es la de \
                 {y} o partidas que no suman lo que se reparte): comprueba en ella que se dotó.",
                del(a)
            ));
            ficheros.push(acta.to_owned());
        }
        None => match (pendiente, limite) {
            (true, Some(l)) => detalle.push_str(&format!(
                " Pendiente: la junta tiene de plazo hasta el {}.",
                fecha(l)
            )),
            (false, Some(l)) => detalle.push_str(&format!(
                " El plazo de la junta acabó el {}: comprueba en el acta que se dotó (TaxPain no \
                 lo puede saber por la contabilidad).",
                fecha(l)
            )),
            (_, None) => {}
        },
    }
    if m.casillas.contains_key(&c::APLICACION_TOTAL) && dotacion.acta_entera().is_none() {
        let en_el_200 = m.c(c::APLICACION_NIVELACION);
        detalle.push_str(&if en_el_200.cents() > 0 {
            format!(
                " En la aplicación del resultado que declara el propio 200 sólo van {} a la \
                 reserva de nivelación (casilla [01271]).",
                eur(en_el_200)
            )
        } else {
            " En la aplicación del resultado que declara el propio 200 (casilla [01271]) no \
             figura, aunque a veces no se rellena."
                .to_owned()
        });
    }
    detalle.push_str(
        " Si no se dota, hay que pagar el impuesto que se ahorró más un 5 % y los intereses de \
         demora (art. 105.6 LIS)",
    );
    match (ahorrado, riesgo) {
        (Some(a), Some(r)) => detalle.push_str(&format!(
            ": unos {} ({} de impuesto ahorrado más el 5 %; estimación) más los intereses.",
            eur(r),
            eur(a)
        )),
        _ => detalle.push('.'),
    }
    s.add(
        Nuevo {
            codigo: "sociedades.reserva_nivelacion",
            area: Area::Sociedades,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            titulo,
            ficheros,
        }
        .detalle(detalle),
        Some(y),
        // Es un riesgo, no dinero que se recupera: va en el texto, no en el importe.
        None,
    );
    s.pedir(
        Destinatario::Gestoria,
        format!(
            "Confirmar que la junta que aprobó las cuentas de {y} dota la reserva de nivelación \
             ({})",
            eur(niv)
        ),
    );
}

/// La reserva de nivelación está dotada (por las actas del ejercicio, por el balance del 200
/// siguiente o por la aplicación del resultado del propio 200): no hay que comprobar nada, sólo
/// recordar que no se puede repartir hasta que la rebaja se sume a la base.
fn nivelacion_dotada(
    y: i32,
    niv: Money,
    d: &Dotada<'_>,
    dotacion: &Dotacion<'_>,
    fichero: &str,
    s: &mut Salida,
) {
    let mut ficheros = vec![fichero.to_owned()];
    let mut despues_de_la_junta = false;
    let prueba = match d {
        Dotada::Balance {
            antes,
            despues,
            fichero,
        } => {
            ficheros.push((*fichero).to_owned());
            format!(
                "en el balance del 200 de {} la reserva de nivelación (casilla [01002]) pasa de {} \
                 a {}",
                y + 1,
                eur(*antes),
                eur(*despues)
            )
        }
        Dotada::Actas { dotado, actas } => {
            ficheros.extend(actas.iter().map(|(_, f)| (*f).to_owned()));
            let aprobacion = dotacion.acta;
            let otras: Vec<String> = actas
                .iter()
                .filter(|(_, f)| aprobacion.is_none_or(|(_, a)| a != *f))
                .map(|(a, _)| del(a))
                .collect();
            let decisiones = format!(
                "{}{}",
                if otras.len() == 1 {
                    "la decisión"
                } else {
                    "las decisiones"
                },
                y_lista(&otras)
            );
            match aprobacion {
                _ if otras.is_empty() => format!(
                    "el acta{} que aprueba las cuentas de {y} lleva {} a la reserva de nivelación",
                    actas.first().map(|(a, _)| del(a)).unwrap_or_default(),
                    eur(*dotado)
                ),
                Some((a, f)) => {
                    despues_de_la_junta = true;
                    ficheros.push(f.to_owned());
                    if otras.len() == actas.len() {
                        format!(
                            "{decisiones} {} {} a la reserva de nivelación (el acta{} que aprueba \
                             las cuentas no la dotaba)",
                            if otras.len() == 1 { "lleva" } else { "llevan" },
                            eur(*dotado),
                            del(a)
                        )
                    } else {
                        format!(
                            "entre el acta{} que aprueba las cuentas y {decisiones} llevan {} a la \
                             reserva de nivelación",
                            del(a),
                            eur(*dotado)
                        )
                    }
                }
                None => format!(
                    "{decisiones} {} {} a la reserva de nivelación",
                    if otras.len() == 1 { "lleva" } else { "llevan" },
                    eur(*dotado)
                ),
            }
        }
        Dotada::Doscientos { dotado } => format!(
            "la aplicación del resultado que declara el propio 200 lleva {} a la reserva de \
             nivelación (casilla [01271])",
            eur(*dotado)
        ),
    };
    let mut detalle = format!(
        "El 200 de {y} rebajó la base en {} por la reserva de nivelación y {prueba}: la reserva \
         está dotada (art. 105.3 LIS).",
        eur(niv)
    );
    // `TODO(VERIFY)`: el criterio de la DGT sobre si una decisión posterior de la junta que pasa
    // a la reserva de nivelación lo que fue a reservas voluntarias cumple el art. 105.3 LIS.
    if despues_de_la_junta {
        detalle.push_str(
            " Se dotó después de la junta que aprobó las cuentas, y la ley pide dotarla con el \
             resultado del propio ejercicio (art. 105.3 LIS): guarda por escrito el criterio de la \
             gestoría de que así queda cumplido.",
        );
    }
    ficheros.dedup();
    let sumada_en = dotacion.sumada_en;
    match sumada_en {
        Some(z) => detalle.push_str(&format!(
            " La rebaja ya se sumó a la base en el 200 de {z} (casilla [01033]): desde entonces \
             la reserva se puede disponer."
        )),
        None => detalle.push_str(&format!(
            " No se puede repartir hasta que la rebaja se sume a la base: en los cinco años \
             siguientes si alguno sale con base negativa y, lo que quede, en {} (art. 105.2 y \
             105.3 LIS). Si se reparte antes, hay que pagar el impuesto que se ahorró más un 5 % \
             y los intereses de demora (art. 105.6 LIS).",
            y + 5
        )),
    }
    s.add(
        Nuevo {
            codigo: "sociedades.reserva_nivelacion",
            area: Area::Sociedades,
            gravedad: Gravedad::Info,
            responsable: Responsable::Gestoria,
            titulo: format!("Reserva de nivelación de {y}: dotada ({})", eur(niv)),
            ficheros,
        }
        .detalle(detalle),
        Some(y),
        None,
    );
}

/// El acta que aprueba las cuentas, leída entera, no dota la reserva de nivelación (o no
/// entera), ni con las decisiones posteriores del ejercicio. Si con el resultado se podía (iba a
/// reservas voluntarias, dividendos o remanente), es un error que cuesta el impuesto ahorrado más
/// un 5 % e intereses (art. 105.6 LIS). Si todo el resultado se fue a la reserva legal o a
/// compensar pérdidas, la ley deja dotarla con los primeros beneficios de los años siguientes
/// (art. 105.3 LIS): no hay incumplimiento todavía, es un aviso para la junta siguiente.
#[allow(clippy::too_many_arguments)]
fn nivelacion_sin_dotar(
    y: i32,
    m: &Modelo,
    anterior: Option<Money>,
    niv: Money,
    dotacion: &Dotacion<'_>,
    a: &Aplicacion,
    acta: &str,
    fichero: &str,
    s: &mut Salida,
) {
    let dotado = dotacion.de_las_actas();
    let falta = niv - dotado;
    let ahorrado = ahorro_nivelacion(y, m, anterior, dotado);
    let riesgo = ahorrado.map(|a| a.apply_rate(Rate::percent(105)));
    let libre = a.libre();
    let se_podia = libre > UN_EURO;
    let reparto = a.texto();
    let en_el_acta = a.nivelacion();
    let mut detalle = format!(
        "El 200 de {y} rebajó la base en {} por la reserva de nivelación (art. 105.1 LIS), pero \
         el acta{} que aprueba las cuentas de {y} {}.{}",
        eur(niv),
        del(a),
        if en_el_acta.cents() > 0 {
            format!("sólo lleva {} a la reserva de nivelación", eur(en_el_acta))
        } else {
            "no la dota".to_owned()
        },
        if reparto.is_empty() {
            String::new()
        } else {
            format!(" El resultado se reparte así: {reparto}.")
        }
    );
    let posteriores: Vec<String> = dotacion
        .dotan
        .iter()
        .filter(|(_, f)| *f != acta)
        .map(|(b, _)| del(b))
        .collect();
    if !posteriores.is_empty() {
        detalle.push_str(&format!(
            " Con {}{}, son {} de {}.",
            if posteriores.len() == 1 {
                "la decisión"
            } else {
                "las decisiones"
            },
            y_lista(&posteriores),
            eur(dotado),
            eur(niv)
        ));
    }
    let estimacion = |detalle: &mut String| match (ahorrado, riesgo) {
        (Some(ah), Some(r)) => detalle.push_str(&format!(
            ": unos {} ({} de impuesto ahorrado más el 5 %; estimación) más los intereses.",
            eur(r),
            eur(ah)
        )),
        _ => detalle.push('.'),
    };
    let titulo = if se_podia {
        detalle.push_str(&format!(
            " La reserva se dota con el beneficio del mismo ejercicio (art. 105.3 LIS), y lo \
             había: {} van a reservas voluntarias, dividendos o remanente. Si no se dota, en el \
             200 del año del incumplimiento ({}, el de la junta) se suma a la cuota el impuesto \
             que se ahorró más un 5 % y los intereses de demora (art. 105.6 LIS)",
            eur(libre),
            y + 1
        ));
        estimacion(&mut detalle);
        // `TODO(VERIFY)`: si una decisión posterior de la junta que pasa a la reserva de
        // nivelación lo que fue a reservas voluntarias evita el art. 105.6 LIS (criterio de la
        // DGT).
        detalle.push_str(&format!(
            " Qué hacer: díselo a la gestoría cuanto antes. Pregúntale si se puede arreglar con una \
             decisión nueva de la junta que pase {} de las reservas voluntarias a la reserva de \
             nivelación antes de que acabe {}, y pide su criterio por escrito: la ley sólo deja \
             dotarla con beneficios de años posteriores cuando no se pudo con el del propio año.",
            eur(falta),
            y + 1
        ));
        let mut t = format!("Reserva de nivelación de {y}: no se dotó");
        if dotado.cents() > 0 {
            t.push_str(&format!(" entera (faltan {})", eur(falta)));
        } else {
            t.push_str(&format!(" ({})", eur(niv)));
        }
        if let Some(r) = riesgo {
            t.push_str(&format!(", unos {} más intereses", eur(r)));
        }
        t
    } else {
        detalle.push_str(
            " Como todo el resultado va a la reserva legal o a compensar pérdidas, no había \
             beneficio libre para dotarla: la ley deja dotarla con los primeros beneficios de los \
             años siguientes con los que se pueda (art. 105.3 LIS), y mientras tanto no se \
             incumple nada. Si tampoco se dota entonces, en el 200 del año en que no se dote se \
             suma a la cuota el impuesto que se ahorró más un 5 % y los intereses de demora (art. \
             105.6 LIS)",
        );
        estimacion(&mut detalle);
        detalle.push_str(&format!(
            " Qué hacer: que la gestoría la tenga en cuenta en la junta que apruebe las cuentas de \
             {}.",
            y + 1
        ));
        format!(
            "Reserva de nivelación de {y}: pendiente de dotar con los beneficios siguientes ({})",
            eur(falta)
        )
    };
    s.add(
        Nuevo {
            codigo: "sociedades.reserva_nivelacion_sin_dotar",
            area: Area::Sociedades,
            gravedad: if se_podia {
                Gravedad::Error
            } else {
                Gravedad::Aviso
            },
            responsable: Responsable::Gestoria,
            titulo,
            ficheros: vec![fichero.to_owned(), acta.to_owned()],
        }
        .detalle(detalle),
        Some(y),
        // Es un coste si no se arregla, no dinero que se recupera: va en el texto.
        None,
    );
    s.pedir(
        Destinatario::Gestoria,
        if se_podia {
            format!(
                "Dotar la reserva de nivelación de {y} ({}), que el acta de aprobación de las \
                 cuentas no dota, o decir por escrito cómo se regulariza (art. 105 LIS)",
                eur(falta)
            )
        } else {
            format!(
                "Dotar la reserva de nivelación de {y} ({}) con los primeros beneficios de los \
                 años siguientes, en la junta que apruebe las cuentas de {} (art. 105.3 LIS)",
                eur(falta),
                y + 1
            )
        },
    );
}

/// Retenciones de capital mobiliario con dividendos de participadas: si son de esas
/// participadas, no había que retener (art. 128.4.d LIS y art. 61.p RIS) y se recuperan en el
/// impuesto.
fn retenciones(y: i32, m: &Modelo, fichero: &str, s: &mut Salida) {
    let grupo = m.c(c::DIVIDENDOS_GRUPO);
    let ret = m.c(c::RETENCIONES_CAPITAL);
    if grupo.cents() <= 0 || ret < grupo.apply_rate(Rate::percent(10)) {
        return;
    }
    s.add(
        Nuevo {
            codigo: "sociedades.retencion_dividendos",
            area: Area::Sociedades,
            gravedad: Gravedad::Info,
            responsable: Responsable::Gestoria,
            titulo: format!("Retenciones sobre dividendos de participadas en {y}"),
            ficheros: vec![fichero.to_owned()],
        }
        .detalle(format!(
            "El 200 de {y} tiene {} de retenciones del capital mobiliario y {} de dividendos de \
             empresas del grupo y asociadas. Si las retenciones son de esos dividendos, no había \
             que retener (con al menos el 5 % durante un año, art. 61 del Reglamento del IS): se \
             recuperan en el impuesto, pero conviene avisar a las participadas para los próximos \
             repartos.",
            eur(ret),
            eur(grupo)
        )),
        Some(y),
        None,
    );
}

/// ¿Es el cobro de un dividendo, por el concepto del banco? (No la devolución de uno.)
pub(crate) fn es_dividendo(concepto: &str) -> bool {
    let c = crate::documentos::fold(concepto);
    c.contains("dividend") && !c.contains("devol")
}

/// Los movimientos sin los repetidos entre ficheros: el mismo día y el mismo importe cuentan
/// tantas veces como salgan en el fichero que más veces los tenga (dos dividendos iguales el
/// mismo día son dos; el mismo extracto en dos ficheros, uno).
fn sin_repetir(movs: Vec<&MovBanco>) -> Vec<&MovBanco> {
    let mut grupos: BTreeMap<(Date, i64), BTreeMap<&str, Vec<&MovBanco>>> = BTreeMap::new();
    for m in movs {
        grupos
            .entry((m.fecha, m.importe.cents()))
            .or_default()
            .entry(m.origen.as_str())
            .or_default()
            .push(m);
    }
    grupos
        .into_values()
        .filter_map(|por_fichero| por_fichero.into_values().max_by_key(Vec::len))
        .flatten()
        .collect()
}

/// Dividendos cobrados en el banco (el concepto dice «dividendo») en el año en curso o en el
/// anterior, cuyo 200 no está en la carpeta: para el próximo 200, la exención del art. 21 LIS y,
/// si se cobraron con la retención del 19 %, cómo se recupera. Si el plazo de ese 200 ya acabó,
/// ya tenía que estar presentado: hay que comprobar que los recoge y, si no, rectificarlo.
fn dividendos_cobrados(
    exp: &Expediente,
    doscientos: &BTreeMap<i32, (Modelo, String)>,
    plazos: &Plazos200,
    hoy: Date,
    s: &mut Salida,
) {
    if !super::cruces::sociedad(exp) {
        return;
    }
    let (movs, _) = super::cruces::datos::movimientos(exp);
    let cobros = sin_repetir(
        movs.iter()
            .filter(|m| {
                let y = m.fecha.year();
                m.importe > Money::ZERO
                    && (hoy.year() - 1..=hoy.year()).contains(&y)
                    && !doscientos.contains_key(&y)
                    && es_dividendo(&m.concepto)
            })
            .collect(),
    );
    let mut por_año: BTreeMap<i32, Vec<&MovBanco>> = BTreeMap::new();
    for m in cobros {
        por_año.entry(m.fecha.year()).or_default().push(m);
    }
    for (y, mut cobros) in por_año {
        cobros.sort_by_key(|m| m.fecha);
        // El plazo del 200 de `y` (art. 124.1 LIS, con el calendario de la empresa).
        let plazo = plazos.fin(y);
        let vencido = plazo.filter(|p| y < hoy.year() && hoy > p.dia);
        let hasta = vencido.and(plazos.rectificable(y));
        let que_hacer = match (vencido, plazo) {
            _ if y >= hoy.year() => {
                "Qué hacer: díselo a la gestoría antes de cerrar el ejercicio.".to_owned()
            }
            (Some(p), _) => format!(
                "El 200 de {y} ya tenía que estar presentado (el plazo acabó el {}) y no está en \
                 la carpeta. Qué hacer: pide a la gestoría que compruebe que aplica la exención y \
                 que resta la retención, si la hubo; si no, se corrige con una {}{}.",
                p.texto(),
                cauce(y),
                hasta
                    .map(|h| format!(" hasta el {} (art. 66 LGT)", h.texto()))
                    .unwrap_or_default()
            ),
            (None, Some(p)) => format!(
                "Qué hacer: díselo a la gestoría antes de presentar el 200 de {y} (el plazo acaba \
                 el {}).",
                p.texto()
            ),
            (None, None) => "Qué hacer: díselo a la gestoría.".to_owned(),
        };
        // Los que confirman los documentos de las participadas (bruto y retención) y los demás.
        let confirmados = super::participadas::confirmar(exp, &cobros);
        let mut sin: Vec<&MovBanco> = Vec::new();
        let mut con: Vec<(&MovBanco, Confirmado)> = Vec::new();
        for (m, c) in cobros.iter().zip(confirmados) {
            match c {
                Some(c) => con.push((m, c)),
                None => sin.push(m),
            }
        }
        let n_hallazgos = s.hallazgos.len();
        if !con.is_empty() {
            confirmados_con_documentos(
                y,
                &con,
                doscientos,
                plazo,
                vencido.is_some(),
                &que_hacer,
                s,
            );
        }
        if !sin.is_empty() {
            sin_confirmar(y, &sin, vencido.is_some(), &que_hacer, s);
        }
        // Con el 200 ya presentado, el plazo para rectificarlo (el código va en `SIN_RECTIFICAR`
        // por el 200 que aún no se ha presentado).
        if let Some(h) = hasta {
            for x in &mut s.hallazgos[n_hallazgos..] {
                x.rectificable_hasta = Some(h.iso());
                x.rectificable_aprox = !h.exacto;
            }
        }
    }
}

/// «10.000,00 € el 16/03/2026 («TRANSFERENCIA …»)».
fn cobro(m: &MovBanco) -> String {
    format!(
        "{} el {} («{}»)",
        eur(m.importe),
        fecha(m.fecha),
        m.concepto.trim()
    )
}

/// Dividendos cobrados sin documentos de la participada que digan el bruto: un aviso con lo que
/// sería si se cobraron con la retención del 19 %, sin importe.
fn sin_confirmar(y: i32, cobros: &[&MovBanco], vencido: bool, que_hacer: &str, s: &mut Salida) {
    let neto = cobros.iter().fold(Money::ZERO, |a, m| a + m.importe);
    let bruto = cobros
        .iter()
        .fold(Money::ZERO, |a, m| a + bruto_con_retencion(m.importe));
    let lista: Vec<String> = cobros.iter().map(|m| cobro(m)).collect();
    let mut ficheros: Vec<String> = cobros.iter().map(|m| m.origen.clone()).collect();
    ficheros.sort();
    ficheros.dedup();
    s.add(
        Nuevo {
            codigo: "sociedades.dividendos_cobrados",
            area: Area::Sociedades,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            titulo: if vencido {
                format!(
                    "Dividendos cobrados en {y} ({}): comprueba la exención y la retención en \
                     el 200",
                    eur(neto)
                )
            } else {
                format!(
                    "Dividendos cobrados en {y} ({}): la exención y la retención, en el 200",
                    eur(neto)
                )
            },
            ficheros,
        }
        .detalle(format!(
            "En {y} se han cobrado {} con el concepto de dividendo: {}. Si son de una sociedad \
             de la que se tiene al menos el 5 % desde hace un año (o se mantiene hasta \
             cumplirlo), el 95 % está exento en el 200 de {y} (art. 21.1 y 21.10 LIS), y entre \
             sociedades así no se retiene (art. 128.4.d LIS y art. 61.p RIS). Si lo cobrado es \
             lo que queda tras la retención del 19 %, el dividendo bruto es {} y la retención \
             {}: se recupera en el 200 de {y} si el dividendo se contabiliza en bruto, con la \
             retención en la cuenta 473. En la carpeta no hay documentos de la pagadora que lo \
             confirmen (el acta de reparto, su certificado de retenciones o su 193): pídele el \
             certificado del bruto y la retención, que es lo que declara en su 193 de {y} y \
             Hacienda cruza con el 200. Si se tiene al menos el 5 % desde hace un año, \
             comunícaselo con los documentos que lo justifiquen para que no retenga en los \
             próximos repartos (art. 61.p RIS); con menos (unas acciones cotizadas, por \
             ejemplo), la retención del 19 % es correcta y se resta en el 200. {que_hacer}",
            eur(neto),
            super::lista(&lista, 6),
            eur(bruto),
            eur(bruto - neto)
        )),
        Some(y),
        // Sin los documentos de la pagadora, las cifras son condicionales: van en el texto.
        None,
    );
    s.pedir(
        Destinatario::Gestoria,
        if vencido {
            format!(
                "Comprobar que el 200 de {y} aplica la exención del art. 21 LIS a los \
                 dividendos cobrados en {y} ({} en el banco) y resta su retención, si la \
                 hubo; si no, rectificarlo",
                eur(neto)
            )
        } else {
            format!(
                "Contabilizar en bruto los dividendos cobrados en {y} ({} en el banco), con \
                 la retención que lleven, y aplicar la exención del art. 21 LIS en el 200 de \
                 {y}",
                eur(neto)
            )
        },
    );
}

/// Impuesto que se ahorra en el 200 de `y` con la exención de `exento` (el 95 % de los
/// dividendos), con el tipo que le toca a `y` por la cifra de negocios del último 200 de antes
/// (art. 29.1 LIS). La base del resto del año no se conoce (la de otro año puede no parecerse en
/// nada), así que se calcula como si los dividendos fueran toda la base: es el mínimo que se
/// ahorra (con más beneficio, los tramos más altos hacen que sea algo más). Con su explicación.
/// `None` sin ningún 200 de antes.
fn ahorro_exencion(
    y: i32,
    doscientos: &BTreeMap<i32, (Modelo, String)>,
    exento: Money,
) -> Option<(Money, String)> {
    let (ly, (m, _)) = doscientos.range(..y).next_back()?;
    let cifra = m.c(c::CIFRA_NEGOCIOS);
    let reg = regimen(m, y, Some(cifra));
    let base = Money::ZERO;
    let cuota = |b: Money| gross_tax_for(b, reg, y, 365).0;
    let ahorro = cuota(base + exento) - cuota(base);
    let tipo = match reg {
        TaxRegime::General => "el tipo general del 25 %".to_owned(),
        TaxRegime::ReducidaDimension => {
            "el tipo de las de menos de 10 millones de cifra de negocios".to_owned()
        }
        TaxRegime::Micro => "los tipos de las de menos de 1 millón de cifra de negocios".to_owned(),
        TaxRegime::NuevaCreacion => "el 15 % de las de nueva creación".to_owned(),
    };
    Some((
        ahorro,
        format!(
            "estimación mínima: el 95 % del bruto ({}) con {tipo} (art. 29.1 LIS), por la cifra \
             de negocios del 200 de {ly} ({}); con más beneficio en {y}, algo más",
            eur(exento),
            eur(cifra)
        ),
    ))
}

/// Impuesto que corresponde a `gravado` (lo que tributa de los dividendos), sin más base, con el
/// régimen del último 200 anterior a `y`.
fn cuota_dividendos(
    y: i32,
    doscientos: &BTreeMap<i32, (Modelo, String)>,
    gravado: Money,
) -> Option<Money> {
    let (_, (m, _)) = doscientos.range(..y).next_back()?;
    let reg = regimen(m, y, Some(m.c(c::CIFRA_NEGOCIOS)));
    Some(gross_tax_for(gravado.max(Money::ZERO), reg, y, 365).0)
}

/// «PARTICIPADA UNO SL: 12.345,68 € brutos, 2.345,68 € retenidos (19 %) y 10.000,00 € cobrados
/// el 16/03/2026 (la empresa tiene el 40 % según sus documentos desde 2021)».
fn detalle_confirmado(m: &MovBanco, c: &Confirmado) -> String {
    let retenido = if c.retencion.is_zero() {
        "sin retención".to_owned()
    } else {
        format!("{} retenidos (19 %)", eur(c.retencion))
    };
    let pct = match (c.porcentaje_texto(), c.desde) {
        (Some(p), Some(d)) => format!(" (la empresa tiene el {p} según sus documentos desde {d})"),
        (Some(p), None) => format!(" (la empresa tiene el {p} según sus documentos)"),
        _ => String::new(),
    };
    format!(
        "{}: {} brutos, {retenido} y {} cobrados el {}{pct}",
        c.pagadora,
        eur(c.bruto),
        eur(c.neto),
        fecha(m.fecha)
    )
}

/// Dividendos cobrados cuyo bruto y retención confirman los documentos de la participada. Con al
/// menos el 5 % desde antes del año del cobro según esos documentos, la retención no se tenía que
/// practicar (art. 128.4.d LIS y art. 61.p RIS): un error, con la retención como importe, que se
/// recupera en el 200 del año. Si no, o sin retención, un aviso (o una nota) con la retención y
/// la exención.
#[allow(clippy::too_many_arguments)]
fn confirmados_con_documentos(
    y: i32,
    con: &[(&MovBanco, Confirmado)],
    doscientos: &BTreeMap<i32, (Modelo, String)>,
    plazo: Option<Dia>,
    vencido: bool,
    que_hacer: &str,
    s: &mut Salida,
) {
    let suma = |xs: &[&(&MovBanco, Confirmado)], f: fn(&Confirmado) -> Money| {
        xs.iter().fold(Money::ZERO, |a, (_, c)| a + f(c))
    };
    let ficheros_de = |xs: &[&(&MovBanco, Confirmado)]| {
        let mut f: Vec<String> = xs
            .iter()
            .flat_map(|(m, c)| c.ficheros.iter().cloned().chain([m.origen.clone()]))
            .collect();
        f.sort();
        f.dedup();
        f
    };
    // La exención de los del grupo, en el 200 de `y`.
    let del_grupo: Vec<&(&MovBanco, Confirmado)> = con.iter().filter(|(_, c)| c.grupo(y)).collect();
    let exento = suma(&del_grupo, |c| c.bruto).apply_rate(Rate::percent(95));
    let ahorro = (!exento.is_zero())
        .then(|| ahorro_exencion(y, doscientos, exento))
        .flatten();
    let ahorro_texto = match &ahorro {
        Some((a, como)) => format!(
            " Con la exención (casilla [00370]) se dejan de tributar {}: unos {} menos de \
             impuesto ({como}).",
            eur(exento),
            eur(*a)
        ),
        None if !exento.is_zero() => format!(
            " Con la exención (casilla [00370]) se dejan de tributar {}.",
            eur(exento)
        ),
        None => String::new(),
    };
    let doscientos_de = match plazo {
        Some(p) if !vencido => format!("el 200 de {y} (se presenta hasta el {})", p.texto()),
        _ => format!("el 200 de {y}"),
    };
    if !vencido {
        for (m, c) in con {
            s.dividendos.push(super::DividendoConfirmado {
                ejercicio: y,
                fecha: format!(
                    "{:04}-{:02}-{:02}",
                    m.fecha.year(),
                    u8::from(m.fecha.month()),
                    m.fecha.day()
                ),
                pagadora: c.pagadora.clone(),
                neto: c.neto,
                bruto: c.bruto,
                retencion: c.retencion,
                grupo: c.grupo(y),
            });
        }
        if let Some((a, _)) = &ahorro {
            s.exencion_proximo_200 += *a;
        }
        // Lo que tributa de esos dividendos con la exención: el 5 % de los del grupo y el bruto
        // de los demás.
        let gravado = suma(&con.iter().collect::<Vec<_>>(), |c| c.bruto) - exento;
        if let Some(q) = cuota_dividendos(y, doscientos, gravado) {
            s.impuesto_dividendos_proximo_200 += q;
        }
    }
    let mut ahorro_pendiente = Some(ahorro_texto);
    // Del grupo con retención: no se tenía que retener.
    let mal: Vec<&(&MovBanco, Confirmado)> = del_grupo
        .iter()
        .copied()
        .filter(|(_, c)| c.retencion.cents() > 0)
        .collect();
    if !mal.is_empty() {
        let ret = suma(&mal, |c| c.retencion);
        let lista: Vec<String> = mal.iter().map(|(m, c)| detalle_confirmado(m, c)).collect();
        let pagadoras: Vec<String> = mal.iter().map(|(_, c)| c.pagadora.clone()).collect();
        s.add(
            Nuevo {
                codigo: "sociedades.retencion_dividendos_grupo",
                area: Area::Sociedades,
                gravedad: Gravedad::Error,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "Retención del 19 % sobre dividendos del grupo que no se debía practicar: {} \
                     adelantados en {y}",
                    eur(ret)
                ),
                ficheros: ficheros_de(&mal),
            }
            .detalle(format!(
                "Los documentos de las participadas confirman los dividendos cobrados en {y}: {}. \
                 Con al menos el 5 % de la participada durante un año (o manteniéndolo hasta \
                 cumplirlo), el 95 % del dividendo está exento (art. 21.1 y 21.10 LIS) y entre \
                 sociedades así no se retiene (art. 128.4.d LIS y art. 61.p RIS): la retención no \
                 se tenía que practicar y la empresa ha adelantado {} que no le tocaba pagar. No \
                 se pierde: es un pago a cuenta del Impuesto sobre Sociedades de {y} y se resta \
                 en {doscientos_de}; si la cuota sale menor, Hacienda devuelve la diferencia.{} \
                 Qué hacer: pide a la pagadora (a la gestoría, si lleva también las \
                 participadas) el certificado de retenciones de {y} y comunícale la \
                 participación con los documentos que la justifiquen, para que no retenga en los \
                 próximos repartos; en el 200 de {y}, el dividendo en bruto (con la retención en \
                 la cuenta 473), la exención del 95 % y la retención restada.{}",
                lista.join("; "),
                eur(ret),
                ahorro_pendiente.take().unwrap_or_default(),
                if vencido {
                    format!(" {que_hacer}")
                } else {
                    String::new()
                }
            )),
            Some(y),
            Some(ret),
        );
        s.pedir(
            Destinatario::Gestoria,
            format!(
                "Pedir el certificado de retenciones de {y} a {} y comunicarles la participación \
                 (art. 61.p RIS) para que no retengan en los próximos repartos",
                super::lista(&pagadoras, 4)
            ),
        );
        s.pedir(
            Destinatario::Gestoria,
            format!(
                "Restar en el 200 de {y} los {} de retenciones de los dividendos de las \
                 participadas y aplicar la exención del art. 21 LIS (casilla [00370])",
                eur(ret)
            ),
        );
    }
    // Los demás: sin retención, o sin el 5 % acreditado en los documentos.
    let resto: Vec<&(&MovBanco, Confirmado)> = con
        .iter()
        .filter(|(_, c)| !(c.grupo(y) && c.retencion.cents() > 0))
        .collect();
    if resto.is_empty() {
        return;
    }
    let neto = suma(&resto, |c| c.neto);
    let ret = suma(&resto, |c| c.retencion);
    let sin_5 = resto.iter().any(|(_, c)| !c.grupo(y));
    let lista: Vec<String> = resto
        .iter()
        .map(|(m, c)| detalle_confirmado(m, c))
        .collect();
    let exencion = if sin_5 {
        format!(
            " Los documentos no dicen que la empresa tenga al menos el 5 % de {} desde antes de \
             {y}: si lo tiene desde hace un año (o lo mantiene hasta cumplirlo), el 95 % está \
             exento en el 200 de {y} (art. 21.1 y 21.10 LIS) y no se tenía que retener (art. \
             128.4.d LIS y art. 61.p RIS; comunícaselo a la pagadora con los documentos que lo \
             justifiquen); con menos (unas acciones cotizadas, por ejemplo), la retención del 19 \
             % es correcta.",
            super::lista(
                &resto
                    .iter()
                    .filter(|(_, c)| !c.grupo(y))
                    .map(|(_, c)| c.pagadora.clone())
                    .collect::<Vec<_>>(),
                4
            )
        )
    } else {
        " Con al menos el 5 % durante un año, el 95 % está exento (art. 21.1 y 21.10 LIS)."
            .to_owned()
    };
    let retencion = if ret.is_zero() {
        String::new()
    } else {
        format!(
            " La retención ({}) es un pago a cuenta del Impuesto sobre Sociedades de {y}: se \
             resta en {doscientos_de} y, si la cuota sale menor, se devuelve.",
            eur(ret)
        )
    };
    s.add(
        Nuevo {
            codigo: "sociedades.dividendos_confirmados",
            area: Area::Sociedades,
            gravedad: if sin_5 { Gravedad::Aviso } else { Gravedad::Info },
            responsable: Responsable::Gestoria,
            titulo: if ret.is_zero() {
                format!(
                    "Dividendos cobrados en {y} ({}) sin retención: la exención, en el 200",
                    eur(neto)
                )
            } else {
                format!(
                    "Dividendos cobrados en {y} ({}): {} de retención a restar en el 200",
                    eur(neto),
                    eur(ret)
                )
            },
            ficheros: ficheros_de(&resto),
        }
        .detalle(format!(
            "Los documentos de las participadas confirman los dividendos cobrados en {y}: {}.{}{}{} \
             {que_hacer}",
            lista.join("; "),
            exencion,
            retencion,
            ahorro_pendiente.take().unwrap_or_default(),
        )),
        Some(y),
        Some(ret),
    );
    s.pedir(
        Destinatario::Gestoria,
        format!(
            "Contabilizar en bruto los dividendos cobrados en {y} ({} en el banco), restar su \
             retención en el 200 de {y} y, si se tiene al menos el 5 % desde hace un año, \
             aplicar la exención del art. 21 LIS",
            eur(neto)
        ),
    );
}

/// Bases imponibles negativas compensadas de más: la cadena de los 200 de la carpeta. Si falta
/// el 200 de algún año, lo que quedaba antes ya no sirve (en los años que faltan se pudo
/// compensar o generar más): la cadena empieza de nuevo, como con el primer 200 de la carpeta.
fn bases_negativas(doscientos: &BTreeMap<i32, (Modelo, String)>, s: &mut Salida) {
    // `None`: no se sabe cuánto queda (había bases de antes de los 200 de la cadena).
    let mut pendiente: Option<Money> = None;
    let mut anterior: Option<i32> = None;
    // El primer año de la cadena y, si empieza tras un hueco, el último 200 de antes y lo que
    // dejaba pendiente.
    let mut inicio: Option<i32> = None;
    let mut antes_del_hueco: Option<(i32, Money)> = None;
    for (y, (m, fichero)) in doscientos {
        if let Some(a) = anterior
            && a != y - 1
        {
            antes_del_hueco = pendiente.filter(|p| !p.is_zero()).map(|p| (a, p));
            pendiente = None;
            inicio = None;
        }
        let desde = *inicio.get_or_insert(*y);
        let usada = m.c(c::BIN_COMPENSADAS);
        if let Some(p) = pendiente
            && usada > p + UN_EURO
        {
            let exceso = usada - p;
            let cifra_anterior = doscientos
                .get(&(y - 1))
                .map(|(m, _)| m.c(c::CIFRA_NEGOCIOS));
            let importe =
                ahorro(*y, m, cifra_anterior, Money::ZERO, Some(p)).filter(|a| !a.is_zero());
            let mut detalle = format!(
                "El 200 de {y} compensa {} de bases imponibles negativas, pero según los 200 de \
                 la carpeta (desde {desde}) sólo quedaban {}: {} de más. La base negativa que se \
                 arrastra es la base imponible del 200 (no la pérdida contable).",
                eur(usada),
                eur(p),
                eur(exceso)
            );
            if let Some(a) = importe {
                detalle.push_str(&format!(
                    " Corregido, la cuota sube unos {} (estimación); conviene regularizarlo en la \
                     misma {} que otras correcciones a favor.",
                    eur(-a),
                    cauce(*y)
                ));
            }
            match antes_del_hueco {
                Some((a, q)) => detalle.push_str(&format!(
                    " Si quedaban bases negativas de antes de {desde}, puede estar bien: los 200 \
                     hasta el de {a} dejaban {} pendientes y {} en la carpeta.",
                    eur(q),
                    match desde - 1 - a {
                        1 => format!("falta el de {}", a + 1),
                        2 => format!("faltan los de {} y {}", a + 1, a + 2),
                        _ => format!("faltan los de {} a {}", a + 1, desde - 1),
                    }
                )),
                None => detalle.push_str(
                    " Si había bases negativas de ejercicios anteriores a los 200 de la carpeta, \
                     puede estar bien.",
                ),
            }
            s.add(
                Nuevo {
                    codigo: "sociedades.bases_negativas",
                    area: Area::Sociedades,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Gestoria,
                    titulo: format!("Bases negativas compensadas de más en {y}: {}", eur(exceso)),
                    ficheros: vec![fichero.clone()],
                }
                .detalle(detalle),
                Some(*y),
                importe,
            );
            s.pedir(
                Destinatario::Gestoria,
                format!(
                    "Revisar la compensación de bases negativas del Impuesto sobre Sociedades de \
                     {y} ({} de más según los 200 anteriores)",
                    eur(exceso)
                ),
            );
        }
        let base = base_final(m);
        let generada = if base.cents() < 0 { -base } else { Money::ZERO };
        pendiente = match pendiente {
            Some(p) => Some((p - usada).max(Money::ZERO) + generada),
            None if usada.is_zero() => Some(generada),
            None => None,
        };
        anterior = Some(*y);
    }
}

/// Hallazgos que proponen rectificar el 200 de su ejercicio.
const RECTIFICAN: &[&str] = &[
    "sociedades.exencion_dividendos",
    "sociedades.bases_negativas",
    "sociedades.reserva_nivelacion_tope",
];

/// Por qué cambia al rectificar uno anterior un 200 en el que la revisión no ve nada que corregir
/// por sí mismo: suma la reserva de nivelación de años anteriores ([01033]), compensa bases
/// negativas ([00547]) o las deja (base negativa). Vacío si no hace nada de eso.
fn arrastra(m: &Modelo) -> Vec<String> {
    let mut por_que = Vec::new();
    if m.c(c::NIVELACION_ADICION).cents() > 0 {
        por_que
            .push("suma la reserva de nivelación de años anteriores (casilla [01033])".to_owned());
    }
    if m.c(c::BIN_COMPENSADAS).cents() > 0 {
        por_que.push("compensa bases negativas (casilla [00547])".to_owned());
    }
    if base_final(m).cents() < 0 {
        por_que.push("deja bases negativas".to_owned());
    }
    por_que
}

/// «a», «a y b», «a, b y c».
fn lista_y(cosas: &[String]) -> String {
    match cosas {
        [] => String::new(),
        [una] => una.clone(),
        [resto @ .., ultima] => format!("{} y {ultima}", resto.join(", ")),
    }
}

/// «2023», «2023 y 2025», «2023, 2024 y 2025».
fn años(ys: &[i32]) -> String {
    let ys: Vec<String> = ys.iter().map(i32::to_string).collect();
    lista_y(&ys)
}

/// Si la revisión propone rectificar el 200 de varios años, el orden: primero el más antiguo,
/// porque al rectificarlo cambian su base, su reserva de nivelación y las bases negativas que
/// deja, y con ellas lo que suman ([01033]) o compensan ([00547]) los siguientes. Los 200 de en
/// medio en los que la revisión no ve nada que corregir también cambian si arrastran algo de los
/// anteriores, o si tienen base positiva y la rectificación de uno anterior deja bases negativas
/// que pueden compensar (art. 26 LIS). Las cifras de cada hallazgo salen de los 200 tal como se
/// presentaron.
fn orden_rectificativas(
    doscientos: &BTreeMap<i32, (Modelo, String)>,
    plazos: &Plazos200,
    hoy: Date,
    s: &mut Salida,
) {
    let propuestos: std::collections::BTreeSet<i32> = s
        .hallazgos
        .iter()
        .filter(|h| RECTIFICAN.contains(&h.codigo.as_str()) && h.gravedad != Gravedad::Info)
        .filter_map(|h| h.ejercicio)
        .filter(|y| plazos.rectificable(*y).is_some_and(|h| hoy <= h.dia))
        .collect();
    let (Some(&primero), Some(&ultimo)) = (propuestos.first(), propuestos.last()) else {
        return;
    };
    if primero == ultimo {
        return;
    }
    let cero = Money::ZERO;
    let cifra = |y: i32| {
        doscientos
            .get(&(y - 1))
            .map(|(m, _)| m.c(c::CIFRA_NEGOCIOS))
    };
    let mut cadena = Vec::new();
    let mut tambien: Vec<(i32, Vec<String>)> = Vec::new();
    let mut faltan = Vec::new();
    // Las bases negativas que dejan las rectificaciones de antes y que todavía no ha compensado
    // ningún 200 posterior (estimación), y los años que las dejan.
    let mut nuevas = cero;
    let mut dejan: Vec<i32> = Vec::new();
    for y in primero..=ultimo {
        let m = doscientos.get(&y).map(|(m, _)| m);
        if propuestos.contains(&y) {
            cadena.push(y);
            if let Some(m) = m {
                let base = (base_final(m) - exento_que_falta(m).unwrap_or(cero)).max(cero);
                nuevas -= nuevas.min(base);
                let deja = bases_que_deja(y, m, cifra(y));
                if deja.cents() > 0 {
                    nuevas += deja;
                    dejan.push(y);
                }
            }
            continue;
        }
        let Some(m) = m else {
            faltan.push(y);
            continue;
        };
        let mut por_que = arrastra(m);
        let base = base_final(m).max(cero);
        if nuevas.cents() > 0 && base.cents() > 0 {
            por_que.push(format!(
                "puede compensar las bases negativas que deja {} (art. 26 LIS)",
                match dejan.as_slice() {
                    [uno] => format!("la rectificación del de {uno}"),
                    varios => format!("rectificar los de {}", años(varios)),
                }
            ));
            nuevas -= nuevas.min(base);
        }
        if nuevas.is_zero() {
            dejan.clear();
        }
        if !por_que.is_empty() {
            cadena.push(y);
            tambien.push((y, por_que));
        }
    }
    let lista: Vec<i32> = propuestos.iter().copied().collect();
    let mut detalle = format!(
        "La revisión propone rectificar los 200 de {}. Hay que hacerlo por orden, del más antiguo \
         al más reciente: al rectificar un 200 cambian su base imponible, su reserva de \
         nivelación (casilla [01034]) y las bases negativas que deja, y con ellas lo que suman o \
         compensan los siguientes (casillas [01033] y [00547]).",
        años(&lista)
    );
    match tambien.as_slice() {
        [] => {}
        [(uno, por_que)] => detalle.push_str(&format!(
            " La revisión no ve nada que corregir en el 200 de {uno} por sí mismo, pero {}: \
             también cambia, y hay que rectificarlo después del de {} y antes del de {}.",
            lista_y(por_que),
            cadena
                .iter()
                .rev()
                .find(|y| **y < *uno)
                .copied()
                .unwrap_or(primero),
            cadena
                .iter()
                .find(|y| **y > *uno)
                .copied()
                .unwrap_or(ultimo)
        )),
        varios => detalle.push_str(&format!(
            " La revisión no ve nada que corregir por sí mismos en los 200 de {}, pero también \
             cambian ({}): van en su lugar en el orden.",
            años(&varios.iter().map(|(y, _)| *y).collect::<Vec<_>>()),
            varios
                .iter()
                .map(|(y, por_que)| format!("el de {y} {}", lista_y(por_que)))
                .collect::<Vec<_>>()
                .join("; ")
        )),
    }
    if !faltan.is_empty() {
        detalle.push_str(&format!(
            " {} en la carpeta: si suma la reserva de nivelación o compensa bases negativas, \
             también cambia.",
            if faltan.len() == 1 {
                format!("Falta el 200 de {}", faltan[0])
            } else {
                format!("Faltan los 200 de {}", años(&faltan))
            }
        ));
    }
    detalle.push_str(
        " Las cifras de cada hallazgo están calculadas con los 200 tal como se presentaron: al \
         rectificar los anteriores, cambian (sobre todo las bases negativas que quedan para \
         compensar).",
    );
    let orden = años(&cadena);
    s.add(
        Nuevo {
            codigo: "sociedades.orden_rectificativas",
            area: Area::Sociedades,
            gravedad: Gravedad::Info,
            responsable: Responsable::Gestoria,
            titulo: format!("Rectificar el Impuesto sobre Sociedades por orden: {orden}"),
            ficheros: cadena
                .iter()
                .filter_map(|y| doscientos.get(y).map(|(_, f)| f.clone()))
                .collect(),
        }
        .detalle(detalle),
        Some(primero),
        None,
    );
    s.pedir(
        Destinatario::Gestoria,
        format!(
            "Presentar las rectificaciones del Impuesto sobre Sociedades por orden, del más \
             antiguo al más reciente ({orden}), cada una con la anterior ya corregida"
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modelo(y: i32, casillas: &[(u32, i64)]) -> Modelo {
        Modelo {
            modelo: "200".into(),
            ejercicio: y,
            periodo: "0A".into(),
            casillas: casillas
                .iter()
                .map(|(k, v)| (*k, Money::from_cents(*v)))
                .collect(),
            ..Modelo::default()
        }
    }

    /// El tipo se elige con la cifra de negocios del ejercicio anterior (art. 29.1 LIS): una SL
    /// que pasa de 900.000 € a 1,2 M€ tributa el año del salto todavía como microempresa.
    #[test]
    fn el_tipo_mira_la_cifra_de_negocios_del_ejercicio_anterior() {
        let m = modelo(
            2025,
            &[
                (c::CIFRA_NEGOCIOS, 120_000_000),
                (c::BASE_PREVIA, 10_000_000),
            ],
        );
        // Sin el 200 anterior en la carpeta, con la propia cifra: reducida dimensión, 24 %.
        assert_eq!(
            calcular(2025, &m, None, Money::ZERO, None).cuota,
            Money::from_cents(2_400_000)
        );
        // Con 900.000 € el año anterior: microempresa, 21 % / 22 % en 2025.
        assert_eq!(
            calcular(
                2025,
                &m,
                Some(Money::from_cents(90_000_000)),
                Money::ZERO,
                None
            )
            .cuota,
            Money::from_cents(1_050_000 + 1_100_000)
        );
    }

    /// Una entidad de nueva creación tributa al 15 % (art. 29.1 LIS): se reconoce por la marca
    /// [00071] del 200 o por el tipo de gravamen [00558] = 15,00.
    #[test]
    fn nueva_creacion_al_15_por_la_marca_o_por_el_tipo() {
        let base = &[
            (c::CIFRA_NEGOCIOS, 50_000_000),
            (c::BASE_PREVIA, 10_000_000),
        ];
        assert_eq!(regimen(&modelo(2025, base), 2025, None), TaxRegime::Micro);
        let mut con_marca = base.to_vec();
        con_marca.push((c::NUEVA_CREACION, 100));
        let m = modelo(2025, &con_marca);
        assert_eq!(regimen(&m, 2025, None), TaxRegime::NuevaCreacion);
        assert_eq!(
            calcular(2025, &m, None, Money::ZERO, None).cuota,
            Money::from_cents(1_500_000)
        );
        let mut con_tipo = base.to_vec();
        con_tipo.push((c::TIPO_GRAVAMEN, 1500));
        assert_eq!(
            regimen(&modelo(2025, &con_tipo), 2025, None),
            TaxRegime::NuevaCreacion
        );
        // Otro tipo en la [00558] no cambia nada.
        let mut otro = base.to_vec();
        otro.push((c::TIPO_GRAVAMEN, 2500));
        assert_eq!(regimen(&modelo(2025, &otro), 2025, None), TaxRegime::Micro);
    }

    /// El tope de 1 M€ de la reserva de nivelación se prorratea en un periodo corto (art. 105.1
    /// LIS): del 1 de julio al 31 de diciembre (184 días), 504.109,59 €.
    #[test]
    fn el_tope_de_la_nivelacion_se_prorratea_en_periodos_cortos() {
        assert_eq!(tope_nivelacion(365), Money::from_euros(1_000_000, 0));
        assert_eq!(tope_nivelacion(366), Money::from_euros(1_000_000, 0));
        assert_eq!(tope_nivelacion(0), Money::from_euros(1_000_000, 0));
        assert_eq!(tope_nivelacion(184), Money::from_cents(50_410_959));
        // Base de 20 M€ tras bases negativas con la reserva al tope: en un año entero, 1 M€;
        // en medio año, el tope prorrateado (y el tramo de 50.000 € de las micro, también).
        let mut m = modelo(
            2025,
            &[
                (c::CIFRA_NEGOCIOS, 50_000_000),
                (c::BASE_PREVIA, 2_000_000_000),
                (c::NIVELACION, 100_000_000),
            ],
        );
        assert_eq!(
            calcular(2025, &m, None, Money::ZERO, None).base,
            Money::from_euros(19_000_000, 0)
        );
        m.periodo_desde = Some(Date::from_calendar_date(2025, Month::July, 1).unwrap());
        m.periodo_hasta = Some(Date::from_calendar_date(2025, Month::December, 31).unwrap());
        assert_eq!(m.dias_periodo(), 184);
        let c = calcular(2025, &m, None, Money::ZERO, None);
        assert_eq!(
            c.base,
            Money::from_euros(20_000_000, 0) - Money::from_cents(50_410_959)
        );
        // Y la revisión avisa de que el 200 aplica más de lo que permite el tope.
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 3, 1),
            &mut s,
        );
        assert!(
            s.hallazgos
                .iter()
                .any(|h| h.codigo == "sociedades.reserva_nivelacion_tope"),
            "{:?}",
            s.hallazgos.iter().map(|h| &h.codigo).collect::<Vec<_>>()
        );
    }

    /// Si la liquidación del justificante no cuadró al leerlo, el resultado no se sabe: no es
    /// cero.
    #[test]
    fn el_resultado_que_no_se_ha_podido_leer_no_es_cero() {
        let mut m = modelo(2023, &[(c::RESULTADO_LIQUIDACION, 999)]);
        m.dudosas = [599, 611, c::A_INGRESAR_DEVOLVER, c::RESULTADO_LIQUIDACION]
            .into_iter()
            .collect();
        let bien = modelo(2024, &[(c::A_INGRESAR_DEVOLVER, -123_456)]);
        let doscientos = BTreeMap::from([
            (2023, (m, "200-2023.pdf".to_owned())),
            (2024, (bien, "200-2024.pdf".to_owned())),
        ]);
        let mut s = Salida::default();
        tabla(&doscientos, &mut s);
        let filas = &s.tablas[0].filas;
        assert_eq!(filas[0].last().unwrap(), "sin leer");
        assert_eq!(filas[1].last().unwrap(), "-1.234,56 €");
    }

    /// Desde 2024 la corrección a favor va por autoliquidación rectificativa (art. 120.4 LGT);
    /// antes, por solicitud de rectificación (art. 120.3 LGT).
    #[test]
    fn el_cauce_de_la_rectificacion_depende_del_ejercicio() {
        assert!(
            cauce(2023).contains("solicitud de rectificación") && cauce(2023).contains("120.3")
        );
        assert!(
            cauce(2024).contains("autoliquidación rectificativa") && cauce(2024).contains("120.4")
        );
    }

    /// Un 200 de 2023 de una micro (23 %) con dividendos del grupo sin exención, reserva de
    /// capitalización en su límite, bases negativas y nivelación: el cálculo reproduce la cuota
    /// presentada y, con la exención, baja lo que sale a mano. Cifras inventadas.
    #[test]
    fn exencion_que_falta_en_2023() {
        // Base previa 40.000; capitalización 4.000 (el 10 %); bases negativas 6.000;
        // nivelación 3.000 (el 10 % de 30.000); base 27.000; cuota al 23 %: 6.210.
        let m = modelo(
            2023,
            &[
                (c::CIFRA_NEGOCIOS, 50_000_000),
                (c::DIVIDENDOS, 1_000_000),
                (c::DIVIDENDOS_GRUPO, 1_000_000),
                (c::BASE_PREVIA, 4_000_000),
                (c::CAPITALIZACION, 400_000),
                (c::BIN_COMPENSADAS, 600_000),
                (c::BASE_TRAS_BIN, 3_000_000),
                (c::NIVELACION, 300_000),
                (c::BASE_IMPONIBLE, 2_700_000),
                (c::CUOTA_INTEGRA, 621_000),
            ],
        );
        let original = calcular(2023, &m, None, Money::ZERO, None);
        assert_eq!(original.base, Money::from_cents(2_700_000));
        assert_eq!(original.cuota, Money::from_cents(621_000));
        // Con 9.500 exentos (el 95 % de 10.000): base previa 30.500, capitalización 3.050,
        // bases negativas 6.000, nivelación 2.145 y base 19.305; cuota 4.440,15.
        let a = ahorro(2023, &m, None, Money::from_cents(950_000), None).unwrap();
        assert_eq!(a, Money::from_cents(176_985));
    }

    /// Faltan los 200 de 2020 y 2021: las bases que dejaban los de 2018 y 2019 (41.000 €) se
    /// pudieron compensar entonces, así que la cadena empieza de nuevo en 2022 (8.000 €; 3.000 €
    /// compensados en 2023 y 1.500 € más en 2024): en 2025 se compensan 7.000 € y quedaban
    /// 6.500 €. Antes se arrastraban los 41.000 € por encima del hueco y no salía nada.
    #[test]
    fn bases_negativas_con_un_hueco_en_los_200() {
        let doscientos: BTreeMap<i32, (Modelo, String)> = [
            (2018, vec![(c::BASE_IMPONIBLE, -4_000_000)]),
            (2019, vec![(c::BASE_IMPONIBLE, -100_000)]),
            (2022, vec![(c::BASE_IMPONIBLE, -800_000)]),
            (
                2023,
                vec![
                    (c::BASE_PREVIA, 300_000),
                    (c::BIN_COMPENSADAS, 300_000),
                    (c::BASE_IMPONIBLE, 0),
                ],
            ),
            (2024, vec![(c::BASE_IMPONIBLE, -150_000)]),
            (
                2025,
                vec![
                    (c::BASE_PREVIA, 2_000_000),
                    (c::BIN_COMPENSADAS, 700_000),
                    (c::BASE_IMPONIBLE, 1_300_000),
                ],
            ),
        ]
        .into_iter()
        .map(|(y, casillas)| (y, (modelo(y, &casillas), format!("{y}/200.pdf"))))
        .collect();
        let mut s = Salida::default();
        bases_negativas(&doscientos, &mut s);
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.bases_negativas")
            .unwrap();
        assert_eq!(h.ejercicio, Some(2025));
        assert!(h.titulo.contains("500,00 €"), "{}", h.titulo);
        assert!(
            h.detalle.contains("(desde 2022) sólo quedaban 6.500,00 €"),
            "{}",
            h.detalle
        );
        assert!(
            h.detalle.contains(
                "los 200 hasta el de 2019 dejaban 41.000,00 € pendientes y faltan los de 2020 y \
                 2021 en la carpeta"
            ),
            "{}",
            h.detalle
        );
    }

    /// 2025, micro (21 % hasta 50.000 y 22 % el resto): la exención y las bases negativas de
    /// más, por separado y a la vez. Cifras inventadas.
    #[test]
    fn exencion_y_bases_negativas_en_2025() {
        // Base previa 120.000; bases negativas 5.000; nivelación 11.500 (el 10 % de 115.000);
        // base 103.500; cuota 10.500 + 11.770 = 22.270.
        let m = modelo(
            2025,
            &[
                (c::CIFRA_NEGOCIOS, 80_000_000),
                (c::DIVIDENDOS, 2_000_000),
                (c::DIVIDENDOS_GRUPO, 2_000_000),
                (c::BASE_PREVIA, 12_000_000),
                (c::BIN_COMPENSADAS, 500_000),
                (c::BASE_TRAS_BIN, 11_500_000),
                (c::NIVELACION, 1_150_000),
                (c::BASE_IMPONIBLE, 10_350_000),
                (c::CUOTA_INTEGRA, 2_227_000),
            ],
        );
        assert_eq!(
            calcular(2025, &m, None, Money::ZERO, None).cuota,
            Money::from_cents(2_227_000)
        );
        // Exentos 19.000: base 86.400 y cuota 18.508.
        let exento = Money::from_cents(2_000_000).apply_rate(Rate::percent(95));
        assert_eq!(
            ahorro(2025, &m, None, exento, None),
            Some(Money::from_cents(376_200))
        );
        // Sólo quedaban 3.000 de bases negativas: base 105.300 y cuota 22.666.
        assert_eq!(
            ahorro(
                2025,
                &m,
                None,
                Money::ZERO,
                Some(Money::from_cents(300_000))
            ),
            Some(Money::from_cents(-39_600))
        );
        // Las dos cosas: base 88.200 y cuota 18.904.
        assert_eq!(
            ahorro(2025, &m, None, exento, Some(Money::from_cents(300_000))),
            Some(Money::from_cents(336_600))
        );
    }

    /// La casilla 01503 (dividendos de cada participada en el cuadro de participaciones) vacía.
    #[test]
    fn dividendos_sin_detallar_en_el_cuadro_de_participaciones() {
        let base = [
            (c::DIVIDENDOS, 1_000_000),
            (c::DIVIDENDOS_GRUPO, 1_000_000),
            (c::PARTICIPACIONES_NOMINAL, 300_000),
        ];
        // El cuadro relleno y la 01503 vacía: faltan.
        assert_eq!(
            dividendos_sin_detallar(&modelo(2025, &base)),
            Some(Money::from_cents(1_000_000))
        );
        // Con la 01503 rellena, nada.
        let mut con = base.to_vec();
        con.push((c::DIVIDENDOS_PARTICIPADAS, 1_000_000));
        assert_eq!(dividendos_sin_detallar(&modelo(2025, &con)), None);
        // Sin el cuadro leído no se sabe: nada.
        assert_eq!(dividendos_sin_detallar(&modelo(2025, &base[..2])), None);
        // Dividendos de terceros sin exención (menos del 5 %): no van en el cuadro.
        let terceros = [
            (c::DIVIDENDOS, 50_000),
            (c::PARTICIPACIONES_LIBROS, 300_000),
        ];
        assert_eq!(dividendos_sin_detallar(&modelo(2025, &terceros)), None);
        // Si se declararon exentos, sí tienen que estar.
        let exentos = [
            (c::DIVIDENDOS, 50_000),
            (c::EXENCION_RESIDENTES, 47_500),
            (c::PARTICIPACIONES_LIBROS, 300_000),
        ];
        assert_eq!(
            dividendos_sin_detallar(&modelo(2025, &exentos)),
            Some(Money::from_cents(50_000))
        );
    }

    fn hoy(y: i32, m: u8, d: u8) -> Date {
        Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap()
    }

    /// 200 de 2025 de una micro (21 % hasta 50.000): base 30.000, nivelación 3.000 (el 10 %),
    /// base 27.000 y cuota 5.670. Sin la rebaja, 6.300: se ahorraron 630 y, si no se dota la
    /// reserva, se pagan 661,50 más intereses. Cifras inventadas.
    fn con_nivelacion(cuota: i64) -> Modelo {
        modelo(
            2025,
            &[
                (c::CIFRA_NEGOCIOS, 50_000_000),
                (c::BASE_PREVIA, 3_000_000),
                (c::BASE_TRAS_BIN, 3_000_000),
                (c::NIVELACION, 300_000),
                (c::BASE_IMPONIBLE, 2_700_000),
                (c::CUOTA_INTEGRA, cuota),
            ],
        )
    }

    fn hallazgo_nivelacion(s: &Salida) -> &super::super::Hallazgo {
        s.hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.reserva_nivelacion")
            .unwrap()
    }

    #[test]
    fn nivelacion_impuesto_ahorrado_y_riesgo() {
        let m = con_nivelacion(567_000);
        assert_eq!(
            ahorro_nivelacion(2025, &m, None, Money::ZERO),
            Some(Money::from_cents(63_000))
        );
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 3, 1),
            &mut s,
        );
        let h = hallazgo_nivelacion(&s);
        assert!(h.titulo.contains("661,50"), "{}", h.titulo);
        assert!(h.detalle.contains("630,00"), "{}", h.detalle);
        // Un riesgo no es dinero que se recupera: sin importe.
        assert_eq!(h.importe, None);
        // Los artículos buenos: 105.3 (dotar) y 105.6 (si no); el 105.4 es de los pagos
        // fraccionados.
        assert!(h.detalle.contains("105.3") && h.detalle.contains("105.6"));
        assert!(!h.detalle.contains("105.4"));
        // Se sigue pidiendo a la gestoría que lo confirme.
        assert!(s.pedir.iter().any(
            |p| p.a_quien == Destinatario::Gestoria && p.que.contains("reserva de nivelación")
        ));
    }

    #[test]
    fn nivelacion_sin_cifra_si_no_se_reproduce() {
        let m = con_nivelacion(100_000);
        assert_eq!(ahorro_nivelacion(2025, &m, None, Money::ZERO), None);
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 3, 1),
            &mut s,
        );
        let h = hallazgo_nivelacion(&s);
        assert!(!h.titulo.contains("más intereses"), "{}", h.titulo);
        assert!(h.detalle.contains("105.6 LIS)."), "{}", h.detalle);
        assert_eq!(h.importe, None);
    }

    /// Antes del plazo de la junta (30 de junio del año siguiente) está pendiente; después, como
    /// TaxPain no sabe si se dotó, hay que mirar el acta. Las dos, en ámbar.
    #[test]
    fn nivelacion_segun_el_plazo_de_la_junta() {
        let m = con_nivelacion(567_000);
        assert_eq!(
            limite_junta(2025, &m),
            Some(Date::from_calendar_date(2026, Month::June, 30).unwrap())
        );
        let mut antes = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 6, 30),
            &mut antes,
        );
        let h = hallazgo_nivelacion(&antes);
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert!(h.titulo.contains("tiene que dotar"), "{}", h.titulo);
        assert!(h.detalle.contains("Pendiente") && h.detalle.contains("30/06/2026"));
        let mut despues = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 7, 1),
            &mut despues,
        );
        let h = hallazgo_nivelacion(&despues);
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert!(h.titulo.contains("comprueba"), "{}", h.titulo);
        assert!(h.detalle.contains("acta"), "{}", h.detalle);
        // Un ejercicio que acaba el 30 de junio: la junta, hasta el 31 de diciembre.
        let mut corto = m.clone();
        corto.periodo_hasta = Some(Date::from_calendar_date(2025, Month::June, 30).unwrap());
        assert_eq!(
            limite_junta(2025, &corto),
            Some(Date::from_calendar_date(2025, Month::December, 31).unwrap())
        );
    }

    /// Cuando ya no se puede reclamar (cuatro años desde el plazo del 200 del año de la junta),
    /// vuelve a gris y no se pide nada a la gestoría.
    #[test]
    fn nivelacion_prescrita_vuelve_a_informativa() {
        let m = con_nivelacion(567_000);
        // 2025: la junta, en 2026; su 200, hasta el 26/07/2027 (el 25 es domingo); prescribe el
        // 26/07/2031.
        let mut aun = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2031, 7, 26),
            &mut aun,
        );
        assert_eq!(hallazgo_nivelacion(&aun).gravedad, Gravedad::Aviso);
        let mut ya = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2031, 7, 27),
            &mut ya,
        );
        let h = hallazgo_nivelacion(&ya);
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(h.detalle.contains("26/07/2031"), "{}", h.detalle);
        assert!(!h.titulo.contains("intereses"), "{}", h.titulo);
        assert!(ya.pedir.is_empty());
    }

    /// En la revisión entera, la reserva de nivelación no lleva el plazo de rectificación (no hay
    /// nada que rectificar) ni suma a lo recuperable.
    #[test]
    fn nivelacion_sin_plazo_de_rectificar_ni_recuperable() {
        let m = con_nivelacion(567_000);
        let exp = Expediente {
            carpeta: String::new(),
            empresa: "EMPRESA DEMO SL".into(),
            own_nif: Some("B12345674".into()),
            docs: Vec::new(),
            no_admitidos: Vec::new(),
            presentados: vec![crate::presentados::Filed {
                id: 1,
                model: "200".into(),
                year: 2025,
                period: "0A".into(),
                filename: "200.pdf".into(),
                source: "aeat".into(),
                boxes: m
                    .casillas
                    .iter()
                    .map(|(k, v)| (u16::try_from(*k).unwrap(), *v))
                    .collect(),
            }],
            perfil: None,
            perfil_deducido: None,
            taxpain: Default::default(),
            aeat: Vec::new(),
            comprobado: Default::default(),
        };
        let mut s = Salida::default();
        revisar(&exp, hoy(2026, 10, 28), &mut s);
        let r = super::super::cerrar(&exp, s, hoy(2026, 10, 28));
        let h = r
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.reserva_nivelacion")
            .unwrap();
        assert_eq!(h.rectificable_hasta, None);
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert_eq!(r.resumen.recuperable, Money::ZERO);
    }

    fn acta(partidas: &[(&str, i64)]) -> Aplicacion {
        let mut a = Aplicacion {
            ejercicio: Some(2025),
            fecha: Some(hoy(2026, 6, 30)),
            aprueba: true,
            completa: true,
            resultado: Some(Money::from_cents(2_500_000)),
            ..Aplicacion::default()
        };
        for (que, c) in partidas {
            let m = Some(Money::from_cents(*c));
            match *que {
                "legal" => a.reserva_legal = m,
                "voluntarias" => a.reservas_voluntarias = m,
                "nivelacion" => a.reserva_nivelacion = m,
                "compensacion" => a.compensacion_perdidas = m,
                _ => unreachable!(),
            }
        }
        a
    }

    fn con_acta(a: &Aplicacion) -> Dotacion<'_> {
        Dotacion::con_acta(a, "acta.pdf")
    }

    /// El acta que aprueba las cuentas de 2025 lleva el resultado a reserva legal y voluntarias y
    /// no dota la reserva de nivelación: es un error, con el coste del art. 105.6 LIS (661,50 €
    /// más intereses) y qué hacer. Ya no se pide «comprobar» nada.
    #[test]
    fn nivelacion_que_el_acta_no_dota_es_un_error() {
        let m = con_nivelacion(567_000);
        let a = acta(&[("legal", 100_000), ("voluntarias", 2_400_000)]);
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &con_acta(&a),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 10, 31),
            &mut s,
        );
        assert!(
            !s.hallazgos
                .iter()
                .any(|h| h.codigo == "sociedades.reserva_nivelacion")
        );
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.reserva_nivelacion_sin_dotar")
            .unwrap();
        assert_eq!(h.gravedad, Gravedad::Error);
        assert!(
            h.titulo.contains("no se dotó") && h.titulo.contains("661,50"),
            "{}",
            h.titulo
        );
        assert!(h.detalle.contains("30/06/2026"), "{}", h.detalle);
        assert!(
            h.detalle.contains("24.000,00 € a reservas voluntarias"),
            "{}",
            h.detalle
        );
        assert!(
            h.detalle.contains("105.3") && h.detalle.contains("105.6"),
            "{}",
            h.detalle
        );
        // El incumplimiento es del año de la junta, y el arreglo, antes de que acabe.
        assert!(
            h.detalle.contains("(2026, el de la junta)"),
            "{}",
            h.detalle
        );
        assert!(
            h.detalle.contains("antes de que acabe 2026"),
            "{}",
            h.detalle
        );
        assert_eq!(
            h.ficheros,
            vec!["200.pdf".to_owned(), "acta.pdf".to_owned()]
        );
        assert_eq!(h.importe, None);
        assert!(
            s.pedir
                .iter()
                .any(|p| p.que.starts_with("Dotar la reserva de nivelación de 2025"))
        );
        assert!(!s.pedir.iter().any(|p| p.que.starts_with("Confirmar")));
    }

    /// Si el acta sólo dota una parte, el coste es el de lo que falta: 2.000 € de los 3.000 €
    /// (420 € de impuesto, 441 € con el 5 %).
    #[test]
    fn nivelacion_dotada_en_parte() {
        let m = con_nivelacion(567_000);
        let a = acta(&[("nivelacion", 100_000), ("voluntarias", 2_400_000)]);
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &con_acta(&a),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 10, 31),
            &mut s,
        );
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.reserva_nivelacion_sin_dotar")
            .unwrap();
        assert!(h.titulo.contains("faltan 2.000,00 €"), "{}", h.titulo);
        assert!(h.titulo.contains("441,00"), "{}", h.titulo);
        assert!(h.detalle.contains("sólo lleva 1.000,00 €"), "{}", h.detalle);
    }

    /// Si todo el resultado va a la reserva legal o a compensar pérdidas, no había con qué: la
    /// ley deja dotarla con los beneficios de los años siguientes. Aviso, no error.
    #[test]
    fn nivelacion_sin_resultado_libre_es_un_aviso() {
        let m = con_nivelacion(567_000);
        let a = acta(&[("legal", 500_000), ("compensacion", 2_000_000)]);
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &con_acta(&a),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 10, 31),
            &mut s,
        );
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.reserva_nivelacion_sin_dotar")
            .unwrap();
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert!(h.detalle.contains("primeros beneficios"), "{}", h.detalle);
        // La junta de 2026 no incumple nada: ni «no se dotó» ni el año del incumplimiento.
        assert!(
            h.titulo.contains("pendiente de dotar") && !h.titulo.contains("no se dotó"),
            "{}",
            h.titulo
        );
        assert!(!h.detalle.contains("2026, el de la junta"), "{}", h.detalle);
        assert!(h.detalle.contains("105.6 LIS"), "{}", h.detalle);
        assert!(
            s.pedir
                .iter()
                .any(|p| p.que.contains("primeros beneficios de los años siguientes"))
        );
    }

    /// Con el acta que la dota, o con el balance del 200 siguiente en el que la reserva de
    /// nivelación [01002] sube lo minorado, está dotada: una nota, sin «comprueba» ni petición.
    #[test]
    fn nivelacion_dotada_por_el_acta_o_por_el_balance() {
        let m = con_nivelacion(567_000);
        let a = acta(&[("nivelacion", 300_000), ("voluntarias", 2_200_000)]);
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &con_acta(&a),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 10, 31),
            &mut s,
        );
        let h = hallazgo_nivelacion(&s);
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(h.titulo.contains("dotada"), "{}", h.titulo);
        assert!(h.detalle.contains("2030"), "{}", h.detalle);
        assert!(s.pedir.is_empty());
        // Por el balance: 10.000 € de reserva en el 200 de 2025 y 13.000 € en el de 2026.
        let mut antes = m.clone();
        antes
            .casillas
            .insert(c::RESERVA_NIVELACION_BALANCE, Money::from_cents(1_000_000));
        let sig = modelo(2026, &[(c::RESERVA_NIVELACION_BALANCE, 1_300_000)]);
        let d = Dotacion {
            siguiente: Some((&sig, "200-2026.pdf")),
            sumada_en: Some(2027),
            ..Dotacion::default()
        };
        let mut s = Salida::default();
        nivelacion(
            2025,
            &antes,
            &d,
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2027, 8, 1),
            &mut s,
        );
        let h = hallazgo_nivelacion(&s);
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(
            h.detalle.contains("de 10.000,00 € a 13.000,00 €"),
            "{}",
            h.detalle
        );
        assert!(h.detalle.contains("200 de 2027"), "{}", h.detalle);
        assert!(s.pedir.is_empty());
        // Si sube menos de lo minorado, no se sabe: se sigue pidiendo comprobarlo.
        let poco = modelo(2026, &[(c::RESERVA_NIVELACION_BALANCE, 1_100_000)]);
        let d = Dotacion {
            siguiente: Some((&poco, "200-2026.pdf")),
            ..Dotacion::default()
        };
        let mut s = Salida::default();
        nivelacion(
            2025,
            &antes,
            &d,
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2027, 8, 1),
            &mut s,
        );
        assert_eq!(hallazgo_nivelacion(&s).gravedad, Gravedad::Aviso);
        assert!(hallazgo_nivelacion(&s).titulo.contains("comprueba"));
    }

    /// Sin acta ni 200 siguiente, la aplicación del resultado que declara el propio 200 (página
    /// 20): si lleva lo minorado a la reserva de nivelación [01271], está dotada; si no la lleva,
    /// se sigue pidiendo comprobarlo, con la pista.
    #[test]
    fn nivelacion_con_la_aplicacion_del_resultado_del_200() {
        let mut m = con_nivelacion(567_000);
        m.casillas
            .insert(c::APLICACION_TOTAL, Money::from_cents(2_500_000));
        m.casillas
            .insert(c::APLICACION_NIVELACION, Money::from_cents(300_000));
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 10, 31),
            &mut s,
        );
        let h = hallazgo_nivelacion(&s);
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(h.detalle.contains("[01271]"), "{}", h.detalle);
        assert_eq!(h.ficheros, vec!["200.pdf".to_owned()]);
        m.casillas.remove(&c::APLICACION_NIVELACION);
        let mut s = Salida::default();
        nivelacion(
            2025,
            &m,
            &Dotacion::default(),
            None,
            "200.pdf",
            &Plazos200::default(),
            hoy(2026, 10, 31),
            &mut s,
        );
        let h = hallazgo_nivelacion(&s);
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert!(
            h.detalle.contains("casilla [01271]) no figura"),
            "{}",
            h.detalle
        );
    }

    /// Las adiciones [01033] se reparten desde la minoración más antigua y sólo dentro de los
    /// cinco años siguientes.
    #[test]
    fn en_que_200_se_suma_la_nivelacion() {
        let doscientos: BTreeMap<i32, (Modelo, String)> = [
            (2021, modelo(2021, &[(c::NIVELACION, 200_000)])),
            (2023, modelo(2023, &[(c::NIVELACION, 100_000)])),
            (2024, modelo(2024, &[(c::NIVELACION_ADICION, 250_000)])),
            (2025, modelo(2025, &[(c::NIVELACION_ADICION, 50_000)])),
        ]
        .into_iter()
        .map(|(y, m)| (y, (m, format!("200-{y}.pdf"))))
        .collect();
        let sumadas = nivelacion_sumada(&doscientos);
        assert_eq!(sumadas.get(&2021), Some(&2024));
        assert_eq!(sumadas.get(&2023), Some(&2025));
    }

    fn movimiento(f: Date, centimos: i64, concepto: &str, origen: &str) -> MovBanco {
        MovBanco {
            fecha: f,
            importe: Money::from_cents(centimos),
            concepto: concepto.into(),
            origen: origen.into(),
            cuenta: String::new(),
        }
    }

    fn con_movimientos(
        movs: Vec<MovBanco>,
        presentados: Vec<crate::presentados::Filed>,
    ) -> Expediente {
        Expediente {
            carpeta: String::new(),
            empresa: "EMPRESA DEMO SL".into(),
            own_nif: Some("B12345674".into()),
            docs: Vec::new(),
            no_admitidos: Vec::new(),
            presentados,
            perfil: None,
            perfil_deducido: None,
            taxpain: super::super::cruces::datos::DatosTaxPain {
                movimientos: movs,
                ..Default::default()
            },
            aeat: Vec::new(),
            comprobado: Default::default(),
        }
    }

    /// Dos dividendos de 3.402 € el mismo día (lo que queda de 4.200 € con el 19 % retenido):
    /// una nota para el 200 de 2026 con la exención y la retención que se recupera. No cuentan
    /// la devolución de un dividendo, el que se paga al socio ni el mismo movimiento repetido en
    /// otro extracto.
    #[test]
    fn dividendos_cobrados_para_el_proximo_200() {
        let d = hoy(2026, 5, 14);
        let exp = con_movimientos(
            vec![
                movimiento(
                    d,
                    340_200,
                    "TRANSFERENCIA PARTICIPADA UNO SL DIVIDENDO",
                    "banco.pdf",
                ),
                movimiento(
                    d,
                    340_200,
                    "TRANSFERENCIA PARTICIPADA DOS SL DIVIDENDO",
                    "banco.pdf",
                ),
                movimiento(
                    d,
                    340_200,
                    "TRANSF. PARTICIPADA UNO DIVIDENDO",
                    "banco.xlsx",
                ),
                movimiento(d, 50_000, "DEVOLUCION DIVIDENDO SOCIO", "banco.pdf"),
                movimiento(d, -200_000, "DIVIDENDO AL SOCIO", "banco.pdf"),
                movimiento(d, 120_000, "COBRO FACTURA 7", "banco.pdf"),
            ],
            Vec::new(),
        );
        let mut s = Salida::default();
        revisar(&exp, hoy(2026, 10, 31), &mut s);
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.dividendos_cobrados")
            .unwrap();
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert_eq!(h.ejercicio, Some(2026));
        assert!(h.titulo.contains("6.804,00 €"), "{}", h.titulo);
        assert!(h.detalle.contains("bruto es 8.400,00 €"), "{}", h.detalle);
        assert!(h.detalle.contains("retención 1.596,00 €"), "{}", h.detalle);
        assert!(
            h.detalle.contains("art. 21.1") && h.detalle.contains("61.p"),
            "{}",
            h.detalle
        );
        assert!(!h.detalle.contains("DEVOLUCION"), "{}", h.detalle);
        assert!(
            s.pedir
                .iter()
                .any(|p| p.que.contains("dividendos cobrados en 2026"))
        );
        // Con el 200 de 2026 ya presentado, ya no es para el próximo 200.
        let presentado = crate::presentados::Filed {
            id: 1,
            model: "200".into(),
            year: 2026,
            period: "0A".into(),
            filename: "200.pdf".into(),
            source: "aeat".into(),
            boxes: [(500, Money::from_cents(100))].into_iter().collect(),
        };
        let exp = con_movimientos(exp.taxpain.movimientos.clone(), vec![presentado]);
        let mut s = Salida::default();
        revisar(&exp, hoy(2027, 8, 1), &mut s);
        assert!(
            !s.hallazgos
                .iter()
                .any(|h| h.codigo == "sociedades.dividendos_cobrados")
        );
        // De hace más de un año, tampoco.
        let mut s = Salida::default();
        revisar(
            &con_movimientos(exp.taxpain.movimientos.clone(), Vec::new()),
            hoy(2028, 1, 10),
            &mut s,
        );
        assert!(
            !s.hallazgos
                .iter()
                .any(|h| h.codigo == "sociedades.dividendos_cobrados")
        );
    }

    /// Un 200 de 2025 con 60.000 € de participaciones del grupo en el balance ([00118] y [00119])
    /// y 40.000 € en el cuadro de participaciones ([01502]): sobran 20.000 €. Cifras inventadas.
    fn con_participaciones(cuadro: Option<i64>) -> Modelo {
        let mut casillas = vec![
            (c::ACTIVO_NO_CORRIENTE, 10_000_000),
            (c::GRUPO_LARGO_PLAZO, 6_000_000),
            (c::GRUPO_PATRIMONIO, 6_000_000),
            (c::PARTICIPACIONES_NOMINAL, 300_000),
        ];
        if let Some(v) = cuadro {
            casillas.push((c::PARTICIPACIONES_LIBROS, v));
        }
        modelo(2025, &casillas)
    }

    fn participaciones(m: &Modelo, hoy: Date) -> Salida {
        let mut s = Salida::default();
        participaciones_del_balance(2025, m, "200.pdf", &Plazos200::default(), hoy, &mut s);
        s
    }

    /// El balance tiene más participaciones del grupo que el cuadro: aviso con la diferencia, el
    /// préstamo al socio mal clasificado (art. 18 LIS) y la participada que falta. Iguales, con
    /// más en el cuadro (participaciones que no son del grupo) o sin el cuadro leído, nada.
    #[test]
    fn participaciones_del_balance_frente_al_cuadro() {
        let s = participaciones(&con_participaciones(Some(4_000_000)), hoy(2026, 10, 31));
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "sociedades.participaciones_balance")
            .unwrap();
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert_eq!(h.ejercicio, Some(2025));
        assert!(h.titulo.contains("20.000,00 €"), "{}", h.titulo);
        assert!(
            h.detalle.contains("60.000,00 €") && h.detalle.contains("40.000,00 €"),
            "{}",
            h.detalle
        );
        assert!(
            h.detalle.contains("[00119]") && h.detalle.contains("[01502]"),
            "{}",
            h.detalle
        );
        assert!(
            h.detalle.contains("préstamo a un socio") && h.detalle.contains("art. 18.1"),
            "{}",
            h.detalle
        );
        assert!(h.detalle.contains("falta una participada"), "{}", h.detalle);
        assert!(s.pedir.iter().any(|p| p.que.contains("20.000,00 €")));
        for cuadro in [
            Some(6_000_000),
            Some(5_999_900),
            Some(6_000_050),
            Some(7_000_000),
            None,
        ] {
            let s = participaciones(&con_participaciones(cuadro), hoy(2026, 10, 31));
            assert!(s.hallazgos.is_empty(), "{cuadro:?}: {:?}", s.hallazgos);
        }
        // Un [00119] mayor que su total [00118] no es del balance (con el mismo número hay
        // casillas de las deducciones por doble imposición): nada.
        let mut otro = con_participaciones(Some(4_000_000));
        otro.casillas
            .insert(c::GRUPO_LARGO_PLAZO, Money::from_cents(1_000_000));
        assert!(
            participaciones(&otro, hoy(2026, 10, 31))
                .hallazgos
                .is_empty()
        );
        // Ya prescrito: nota, sin petición.
        let s = participaciones(&con_participaciones(Some(4_000_000)), hoy(2031, 1, 10));
        assert_eq!(s.hallazgos[0].gravedad, Gravedad::Info);
        assert!(s.pedir.is_empty());
    }

    /// Una carpeta con los 200 de los años dados (cifras inventadas).
    fn con_doscientos(años: &[(i32, Vec<(u32, i64)>)]) -> Expediente {
        Expediente {
            carpeta: String::new(),
            empresa: "EMPRESA DEMO SL".into(),
            own_nif: Some("B12345674".into()),
            docs: Vec::new(),
            no_admitidos: Vec::new(),
            presentados: años
                .iter()
                .enumerate()
                .map(|(i, (y, casillas))| crate::presentados::Filed {
                    id: i64::try_from(i).unwrap(),
                    model: "200".into(),
                    year: *y,
                    period: "0A".into(),
                    filename: format!("200-{y}.pdf"),
                    source: "aeat".into(),
                    boxes: casillas
                        .iter()
                        .map(|(k, v)| (u16::try_from(*k).unwrap(), Money::from_cents(*v)))
                        .collect(),
                })
                .collect(),
            perfil: None,
            perfil_deducido: None,
            taxpain: Default::default(),
            aeat: Vec::new(),
            comprobado: Default::default(),
        }
    }

    fn orden(exp: &Expediente) -> (Option<super::super::Hallazgo>, Vec<String>) {
        let mut s = Salida::default();
        revisar(exp, hoy(2026, 10, 28), &mut s);
        (
            s.hallazgos
                .into_iter()
                .find(|h| h.codigo == "sociedades.orden_rectificativas"),
            s.pedir.into_iter().map(|p| p.que).collect(),
        )
    }

    /// Dividendos sin la exención en 2023 y en 2025: hay que rectificar los dos, por orden. El 200
    /// de 2024 no tiene nada que corregir, pero suma la reserva de nivelación de 2023 y deja
    /// bases negativas: va en medio. Sin él en la carpeta se dice que falta, y si no arrastra
    /// nada no entra en el orden. Con un solo año, no hay orden que dar.
    #[test]
    fn orden_de_las_rectificativas_del_impuesto() {
        let sin_exencion = |y: i32| {
            (
                y,
                vec![
                    (c::DIVIDENDOS, 1_000_000),
                    (c::DIVIDENDOS_GRUPO, 1_000_000),
                    (c::BASE_PREVIA, 3_000_000),
                    (c::BASE_IMPONIBLE, 3_000_000),
                ],
            )
        };
        let arrastra = (
            2024,
            vec![
                (c::NIVELACION_ADICION, 100_000),
                (c::BASE_IMPONIBLE, -50_000),
            ],
        );
        let (h, pedir) = orden(&con_doscientos(&[
            sin_exencion(2023),
            arrastra.clone(),
            sin_exencion(2025),
        ]));
        let h = h.unwrap();
        assert_eq!(h.gravedad, Gravedad::Info);
        assert_eq!(h.ejercicio, Some(2023));
        assert!(h.titulo.ends_with("2023, 2024 y 2025"), "{}", h.titulo);
        assert!(
            h.detalle.contains("rectificar los 200 de 2023 y 2025"),
            "{}",
            h.detalle
        );
        assert!(
            h.detalle
                .contains("La revisión no ve nada que corregir en el 200 de 2024 por sí mismo")
                && h.detalle
                    .contains("después del de 2023 y antes del de 2025"),
            "{}",
            h.detalle
        );
        assert!(
            !h.detalle.contains("no tiene nada que corregir"),
            "{}",
            h.detalle
        );
        assert!(h.detalle.contains("[01033]"), "{}", h.detalle);
        assert_eq!(h.ficheros.len(), 3);
        assert!(
            pedir.iter().any(|p| p.contains("(2023, 2024 y 2025)")),
            "{pedir:?}"
        );
        // Sin el 200 de 2024.
        let (h, _) = orden(&con_doscientos(&[sin_exencion(2023), sin_exencion(2025)]));
        let h = h.unwrap();
        assert!(h.titulo.ends_with("2023 y 2025"), "{}", h.titulo);
        assert!(h.detalle.contains("Falta el 200 de 2024"), "{}", h.detalle);
        // Con un 2024 que no arrastra nada (base positiva, sin bases negativas ni adición).
        let (h, _) = orden(&con_doscientos(&[
            sin_exencion(2023),
            (2024, vec![(c::BASE_IMPONIBLE, 200_000)]),
            sin_exencion(2025),
        ]));
        let h = h.unwrap();
        assert!(h.titulo.ends_with("2023 y 2025"), "{}", h.titulo);
        assert!(!h.detalle.contains("2024"), "{}", h.detalle);
        // Un solo año a rectificar: nada.
        let (h, pedir) = orden(&con_doscientos(&[sin_exencion(2023), arrastra]));
        assert!(h.is_none());
        assert!(!pedir.iter().any(|p| p.contains("por orden")), "{pedir:?}");
        // Si el primero ya no se puede rectificar, tampoco.
        let mut s = Salida::default();
        revisar(
            &con_doscientos(&[sin_exencion(2020), sin_exencion(2023)]),
            hoy(2026, 10, 28),
            &mut s,
        );
        assert!(
            !s.hallazgos
                .iter()
                .any(|h| h.codigo == "sociedades.orden_rectificativas")
        );
    }

    /// Con la exención que falta, la base de 2023 (3.000 €) queda en −6.500 €: la rectificación
    /// deja bases negativas que el 200 de 2024, con base positiva y sin nada que corregir por sí
    /// mismo, puede compensar (art. 26 LIS). También cambia, y va en medio. Si la base de 2023
    /// sigue positiva con la exención, el de 2024 no entra.
    #[test]
    fn la_rectificacion_que_deja_bases_negativas_cambia_los_siguientes() {
        let con_base = |y: i32, base: i64, dividendos: i64| {
            (
                y,
                vec![
                    (c::DIVIDENDOS, dividendos),
                    (c::DIVIDENDOS_GRUPO, dividendos),
                    (c::BASE_PREVIA, base),
                    (c::BASE_IMPONIBLE, base),
                ],
            )
        };
        let (h, pedir) = orden(&con_doscientos(&[
            con_base(2023, 300_000, 1_000_000),
            con_base(2024, 2_000_000, 0),
            con_base(2025, 3_000_000, 1_000_000),
        ]));
        let h = h.unwrap();
        assert!(h.titulo.ends_with("2023, 2024 y 2025"), "{}", h.titulo);
        assert!(
            h.detalle.contains(
                "puede compensar las bases negativas que deja la rectificación del de 2023 (art. \
                 26 LIS)"
            ),
            "{}",
            h.detalle
        );
        assert!(
            pedir.iter().any(|p| p.contains("(2023, 2024 y 2025)")),
            "{pedir:?}"
        );
        let (h, _) = orden(&con_doscientos(&[
            con_base(2023, 3_000_000, 1_000_000),
            con_base(2024, 2_000_000, 0),
            con_base(2025, 3_000_000, 1_000_000),
        ]));
        assert!(h.unwrap().titulo.ends_with("2023 y 2025"));
    }

    /// El plazo del 200 es el del calendario: el 25/07/2026 fue sábado y el de 2025 acabó el
    /// lunes 27, así que se puede rectificar hasta el 27/07/2030 (cuatro años de fecha a fecha,
    /// arts. 66 y 67.1 LGT); el 25/07/2027 es domingo. Con un festivo local el lunes, el martes.
    /// Sin los festivos del año, el 25 de julio, aproximado. Con cierre el 30 de junio, el 200
    /// vence en enero.
    #[test]
    fn el_plazo_del_200_con_el_calendario() {
        let p = Plazos200::default();
        let dia = |y: i32, m: u8, d: u8, exacto: bool| Dia {
            dia: hoy(y, m, d),
            exacto,
        };
        assert_eq!(p.fin(2025), Some(dia(2026, 7, 27, true)));
        assert_eq!(p.rectificable(2025), Some(dia(2030, 7, 27, true)));
        assert_eq!(p.fin(2026), Some(dia(2027, 7, 26, true)));
        assert_eq!(p.rectificable(2026), Some(dia(2031, 7, 26, true)));
        // 2024 sin festivos en TaxPain: el 25 de julio y aproximado.
        assert_eq!(p.fin(2023), Some(dia(2024, 7, 25, false)));
        let r = p.rectificable(2023).unwrap();
        assert_eq!(r, dia(2028, 7, 25, false));
        assert_eq!(r.texto(), "25/07/2028 aprox.");
        assert_eq!(p.rectificable(2025).unwrap().texto(), "27/07/2030");
        assert_eq!(p.rectificable(2025).unwrap().iso(), "2030-07-27");
        let local = Plazos200 {
            perfil: TaxpayerProfile {
                festivos_locales: vec![hoy(2027, 7, 26)],
                ..TaxpayerProfile::sociedad_limitada()
            },
        };
        assert_eq!(local.fin(2026), Some(dia(2027, 7, 27, true)));
        let junio = Plazos200 {
            perfil: TaxpayerProfile {
                cierre_ejercicio: (Month::June, 30),
                ..TaxpayerProfile::sociedad_limitada()
            },
        };
        assert_eq!(junio.fin(2025), Some(dia(2027, 1, 25, true)));
        assert_eq!(junio.rectificable(2025), Some(dia(2031, 1, 25, true)));
    }

    /// En la revisión entera, el plazo de rectificar del 200 de 2025 va con el calendario (exacto)
    /// y el del de 2023, sin los festivos de 2024, aproximado.
    #[test]
    fn el_plazo_de_rectificar_en_la_revision() {
        let sin_exencion = |y: i32| {
            (
                y,
                vec![
                    (c::DIVIDENDOS, 1_000_000),
                    (c::DIVIDENDOS_GRUPO, 1_000_000),
                    (c::BASE_PREVIA, 3_000_000),
                    (c::BASE_IMPONIBLE, 3_000_000),
                ],
            )
        };
        let exp = con_doscientos(&[sin_exencion(2023), sin_exencion(2025)]);
        let mut s = Salida::default();
        revisar(&exp, hoy(2026, 10, 28), &mut s);
        let r = super::super::cerrar(&exp, s, hoy(2026, 10, 28));
        let de = |y: i32| {
            r.hallazgos
                .iter()
                .find(|h| h.codigo == "sociedades.exencion_dividendos" && h.ejercicio == Some(y))
                .unwrap()
        };
        assert_eq!(de(2025).rectificable_hasta.as_deref(), Some("2030-07-27"));
        assert!(!de(2025).rectificable_aprox);
        assert!(
            de(2025)
                .detalle
                .contains("Hasta el 27/07/2030 (art. 66 LGT)")
        );
        assert_eq!(de(2023).rectificable_hasta.as_deref(), Some("2028-07-25"));
        assert!(de(2023).rectificable_aprox);
        assert!(
            de(2023)
                .detalle
                .contains("Hasta el 25/07/2028 aprox. (art. 66 LGT)"),
            "{}",
            de(2023).detalle
        );
    }

    #[test]
    fn bruto_de_lo_cobrado_con_el_19_por_ciento() {
        assert_eq!(
            bruto_con_retencion(Money::from_cents(810_000)),
            Money::from_cents(1_000_000)
        );
        assert_eq!(
            bruto_con_retencion(Money::from_cents(1_380_000)),
            Money::from_cents(1_703_704)
        );
    }

    /// Si el cálculo no reproduce la cuota presentada (otras deducciones, otro tipo…), no hay
    /// cifra.
    #[test]
    fn sin_cifra_si_no_se_reproduce() {
        let m = modelo(
            2025,
            &[
                (c::BASE_PREVIA, 10_000_000),
                (c::BASE_IMPONIBLE, 10_000_000),
                (c::CUOTA_INTEGRA, 100),
            ],
        );
        assert_eq!(
            ahorro(2025, &m, None, Money::from_cents(100_000), None),
            None
        );
    }
}
