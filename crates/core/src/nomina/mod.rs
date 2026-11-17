//! Cálculo de la nómina de un trabajador por cuenta ajena del Régimen General: devengos, bases
//! de cotización con los topes del grupo, cuotas del trabajador y de la empresa, retención del
//! IRPF y líquido. Cálculo puro (sin IO): lo usa `taxpain-app::nominas`.
//!
//! Norma:
//! - Estructura del salario y pagas extraordinarias: arts. 26 a 31 ET (RDLeg 2/2015).
//! - Bases de cotización: art. 147 TRLGSS (RDLeg 8/2015) y arts. 23 y 24 del Reglamento general
//!   de cotización (RD 2064/1995): la remuneración del mes más la prorrata de las pagas
//!   extraordinarias, entre la base mínima del grupo y la máxima. La de contingencias
//!   profesionales (y desempleo, FOGASA y formación profesional) suma las horas
//!   extraordinarias. Tipos, bases y topes del año: [`tablas`] (orden de cotización de cada año).
//! - Conceptos excluidos de la base (dietas, gastos de locomoción, indemnizaciones por
//!   traslado, despido…): art. 147.2 TRLGSS, con los límites del art. 9 RIRPF. TaxPain no
//!   comprueba esos límites: el usuario marca el concepto como «no salarial exento» sólo por la
//!   parte exenta (`TODO(VERIFY)` con su caso).
//! - Retención del IRPF: arts. 80 a 89 RIRPF. TaxPain **no** tiene el algoritmo oficial de la
//!   AEAT verificado: el tipo lo teclea el usuario (lo saca del programa de cálculo de
//!   retenciones de la AEAT, [`PROGRAMA_RETENCIONES`]). [`AlgoritmoRetencion`] deja preparada la
//!   estructura para cuando se implemente y se pueda cotejar con ese programa.
//!
//! Lo que no se calcula (se avisa): la cotización adicional de los contratos temporales de
//! duración inferior a 30 días (art. 151 TRLGSS), las reducciones y bonificaciones de cuotas,
//! la retribución en especie y su ingreso a cuenta, la incapacidad temporal, los convenios
//! colectivos (el salario lo pone el usuario) y la cotización a tiempo parcial por horas (la
//! base mínima se reduce aquí en proporción a la jornada: `TODO(VERIFY)` con las bases mínimas
//! por hora de la orden de cotización).

pub mod finiquito;
pub mod tablas;

use serde::{Deserialize, Serialize};

use crate::money::{Money, Rate};
use tablas::{BaseMinima, Reparto, TablaCotizacion};

/// Programa de ayuda de la AEAT para el cálculo de las retenciones del trabajo.
pub const PROGRAMA_RETENCIONES: &str = "https://sede.agenciatributaria.gob.es/Sede/irpf/retenciones-ingresos-cuenta/calculo-retenciones.html";

/// Modalidad del contrato a efectos del tipo de desempleo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Contrato {
    /// Indefinido (también fijo discontinuo y formativo).
    #[default]
    Indefinido,
    /// De duración determinada.
    Temporal,
}

/// Clase de un concepto del recibo: decide si cotiza y si tributa.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaseConcepto {
    /// Percepción salarial: cotiza y tributa.
    #[default]
    Salarial,
    /// Percepción no salarial que cotiza y tributa (p. ej. la parte de una dieta que excede del
    /// límite exento).
    NoSalarialSujeta,
    /// Percepción no salarial exenta: ni cotiza ni tributa (dietas y locomoción dentro de los
    /// límites del art. 9 RIRPF, indemnizaciones del art. 147.2 TRLGSS).
    NoSalarialExenta,
}

/// Complemento o percepción del mes, con su importe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Concepto {
    pub nombre: String,
    pub importe: Money,
    #[serde(default)]
    pub clase: ClaseConcepto,
}

/// Pagas extraordinarias (art. 31 ET).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagasExtra {
    /// Número de pagas al año (dos como mínimo según el ET; el convenio puede prorratearlas).
    pub numero: u8,
    /// Importe de cada paga completa.
    pub importe: Money,
    /// Se pagan prorrateadas cada mes.
    pub prorrateadas: bool,
    /// Si no están prorrateadas: pagas que se cobran en este recibo (0, 1…).
    #[serde(default)]
    pub en_este_mes: u8,
}

/// Cómo se fija la retención del IRPF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "modo")]
pub enum Retencion {
    /// Tipo tecleado por el usuario (del programa de la AEAT o de la gestoría laboral).
    Manual { tipo: Rate },
}

/// Datos del mes para calcular la nómina.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatosNomina {
    pub year: i32,
    pub month: u8,
    /// Grupo de cotización (1 a 11).
    pub grupo: u8,
    #[serde(default)]
    pub contrato: Contrato,
    /// Días cotizados del mes: 30 con el mes entero en los grupos 1 a 7; en los grupos 8 a 11
    /// (bases diarias), los días naturales del mes.
    #[serde(default = "treinta")]
    pub dias: u8,
    /// Coeficiente de jornada (100 % a jornada completa).
    #[serde(default = "jornada_completa")]
    pub jornada: Rate,
    pub salario_base: Money,
    #[serde(default)]
    pub complementos: Vec<Concepto>,
    #[serde(default)]
    pub horas_extra: Money,
    #[serde(default)]
    pub horas_extra_fuerza_mayor: bool,
    #[serde(default)]
    pub pagas: PagasExtra,
    /// Tarifa de AT/EP de la actividad (DA 4.ª Ley 42/2006): incapacidad temporal…
    pub at_it: Rate,
    /// …e incapacidad permanente, muerte y supervivencia. Las dos, a cargo de la empresa.
    pub at_ims: Rate,
    pub retencion: Retencion,
    #[serde(default)]
    pub anticipos: Money,
    #[serde(default)]
    pub otras_deducciones: Money,
}

fn treinta() -> u8 {
    30
}

fn jornada_completa() -> Rate {
    Rate::percent(100)
}

/// Qué es cada cuota (para sumar las del mes en los seguros sociales).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ClaveCuota {
    Comunes,
    Mei,
    Desempleo,
    Formacion,
    Fogasa,
    AtIt,
    AtIms,
    HorasExtra,
    Solidaridad,
}

impl ClaveCuota {
    pub fn nombre(self) -> &'static str {
        match self {
            ClaveCuota::Comunes => "Contingencias comunes",
            ClaveCuota::Mei => "Mecanismo de equidad intergeneracional (MEI)",
            ClaveCuota::Desempleo => "Desempleo",
            ClaveCuota::Formacion => "Formación profesional",
            ClaveCuota::Fogasa => "FOGASA",
            ClaveCuota::AtIt => "AT y EP: incapacidad temporal",
            ClaveCuota::AtIms => "AT y EP: incapacidad permanente, muerte y supervivencia",
            ClaveCuota::HorasExtra => "Horas extraordinarias",
            ClaveCuota::Solidaridad => "Cotización adicional de solidaridad",
        }
    }
}

/// Una cuota con su base y el reparto entre empresa y trabajador.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuota {
    pub clave: ClaveCuota,
    pub concepto: String,
    pub base: Money,
    pub tipo_empresa: Rate,
    pub empresa: Money,
    pub tipo_trabajador: Rate,
    pub trabajador: Money,
}

/// Línea de devengo del recibo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Devengo {
    pub concepto: String,
    pub importe: Money,
    pub clase: ClaseConcepto,
    /// Gratificación extraordinaria (o su prorrata): va en su línea del recibo.
    #[serde(default)]
    pub extraordinaria: bool,
    #[serde(default)]
    pub horas_extra: bool,
}

/// Tope aplicado a la base de contingencias comunes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tope {
    Minimo,
    Maximo,
}

/// Nómina calculada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nomina {
    pub year: i32,
    pub month: u8,
    pub grupo: u8,
    pub devengos: Vec<Devengo>,
    pub total_devengado: Money,
    /// Remuneración del mes que entra en la base (sin pagas extra ni horas extraordinarias).
    pub remuneracion_mensual: Money,
    pub prorrata_pagas: Money,
    pub base_minima: Money,
    pub base_maxima: Money,
    pub base_comunes: Money,
    pub base_profesionales: Money,
    pub base_horas_extra: Money,
    pub tope: Option<Tope>,
    pub cuotas: Vec<Cuota>,
    pub total_trabajador: Money,
    pub total_empresa: Money,
    pub base_irpf: Money,
    pub tipo_irpf: Rate,
    pub retencion_irpf: Money,
    pub anticipos: Money,
    pub otras_deducciones: Money,
    pub total_deducciones: Money,
    pub liquido: Money,
    /// Total devengado más las cuotas de la empresa.
    pub coste_empresa: Money,
    pub norma: String,
    pub avisos: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NominaError {
    #[error(
        "TaxPain no tiene las bases y tipos de cotización de {0}: sin orden de cotización \
         publicada no se calcula"
    )]
    SinTabla(i32),
    #[error("{0}")]
    Datos(String),
}

fn datos(s: &str) -> NominaError {
    NominaError::Datos(s.into())
}

/// `m × num / den`, redondeado al céntimo (half away from zero).
pub fn proporcion(m: Money, num: i64, den: i64) -> Money {
    if den == 0 {
        return Money::ZERO;
    }
    let n = i128::from(m.cents()) * i128::from(num);
    let d = i128::from(den);
    let q = n / d;
    let r = n % d;
    let q = if 2 * r.abs() >= d.abs() {
        q + n.signum() * d.signum()
    } else {
        q
    };
    Money::from_cents(i64::try_from(q).unwrap_or(i64::MAX))
}

fn cuota(clave: ClaveCuota, base: Money, r: Reparto) -> Cuota {
    Cuota {
        clave,
        concepto: clave.nombre().into(),
        base,
        tipo_empresa: r.empresa,
        empresa: base.apply_rate(r.empresa),
        tipo_trabajador: r.trabajador,
        trabajador: base.apply_rate(r.trabajador),
    }
}

/// Topes de la base del mes: la mínima del grupo (en proporción a los días y a la jornada) y la
/// máxima (en proporción a los días).
pub fn topes(t: &TablaCotizacion, grupo: u8, dias: u8, jornada: Rate) -> Option<(Money, Money)> {
    let dias = i64::from(dias);
    let (min, max) = match t.base_minima(grupo)? {
        BaseMinima::Mensual(m) => (
            proporcion(m, dias.min(30), 30),
            proporcion(t.base_maxima, dias.min(30), 30),
        ),
        BaseMinima::Diaria(d) => (
            Money::from_cents(d.cents() * dias),
            proporcion(t.base_maxima, dias, 30),
        ),
    };
    Some((min.apply_rate(jornada), max))
}

fn validar(d: &DatosNomina) -> Result<(), NominaError> {
    if !(1..=12).contains(&d.month) {
        return Err(datos("el mes va del 1 al 12"));
    }
    if !(1..=11).contains(&d.grupo) {
        return Err(datos("el grupo de cotización va del 1 al 11"));
    }
    let max_dias = if d.grupo <= 7 { 30 } else { 31 };
    if !(1..=max_dias).contains(&d.dias) {
        return Err(NominaError::Datos(format!(
            "los días cotizados van de 1 a {max_dias} en el grupo {}",
            d.grupo
        )));
    }
    if !(1..=10_000).contains(&d.jornada.bp()) {
        return Err(datos("el coeficiente de jornada va del 0,01 al 100 %"));
    }
    let importes = [
        d.salario_base,
        d.horas_extra,
        d.pagas.importe,
        d.anticipos,
        d.otras_deducciones,
    ];
    if importes.iter().any(|m| m.is_negative())
        || d.complementos.iter().any(|c| c.importe.is_negative())
    {
        return Err(datos("los importes no pueden ser negativos"));
    }
    if d.salario_base.is_zero() {
        return Err(datos("falta el salario base"));
    }
    if d.pagas.numero > 6 || d.pagas.en_este_mes > d.pagas.numero {
        return Err(datos(
            "pagas extraordinarias no válidas (como mucho 6, y en el mes no más de las que hay)",
        ));
    }
    if d.pagas.prorrateadas && d.pagas.en_este_mes > 0 {
        return Err(datos(
            "con las pagas prorrateadas no se cobra ninguna entera en el mes",
        ));
    }
    let tipos = [d.at_it, d.at_ims];
    if tipos.iter().any(|r| !(0..=2_000).contains(&r.bp())) {
        return Err(datos("la tarifa de AT y EP va del 0 al 20 %"));
    }
    let Retencion::Manual { tipo } = d.retencion;
    if !(0..=10_000).contains(&tipo.bp()) {
        return Err(datos("el tipo de retención va del 0 al 100 %"));
    }
    Ok(())
}

/// Calcula la nómina del mes con la tabla del año.
pub fn calcular(d: &DatosNomina) -> Result<Nomina, NominaError> {
    let t = tablas::tabla(d.year).ok_or(NominaError::SinTabla(d.year))?;
    calcular_con(t, d)
}

/// Como [`calcular`], con una tabla dada (para las pruebas).
pub fn calcular_con(t: &TablaCotizacion, d: &DatosNomina) -> Result<Nomina, NominaError> {
    validar(d)?;
    let mut avisos = Vec::new();
    let dias = i64::from(d.dias);
    let diaria = d.grupo >= 8;

    // Devengos.
    let mut devengos = vec![Devengo {
        concepto: "Salario base".into(),
        importe: d.salario_base,
        clase: ClaseConcepto::Salarial,
        extraordinaria: false,
        horas_extra: false,
    }];
    for c in d.complementos.iter().filter(|c| !c.importe.is_zero()) {
        devengos.push(Devengo {
            concepto: c.nombre.trim().to_string(),
            importe: c.importe,
            clase: c.clase,
            extraordinaria: false,
            horas_extra: false,
        });
    }
    if !d.horas_extra.is_zero() {
        devengos.push(Devengo {
            concepto: if d.horas_extra_fuerza_mayor {
                "Horas extraordinarias (fuerza mayor)".into()
            } else {
                "Horas extraordinarias".into()
            },
            importe: d.horas_extra,
            clase: ClaseConcepto::Salarial,
            extraordinaria: false,
            horas_extra: true,
        });
    }
    // Prorrata de las pagas: la anual entre 12 y por los días del mes (grupos 1 a 7), o entre
    // 365 por día (8 a 11). `TODO(VERIFY)` el divisor de los grupos de base diaria.
    let anual_pagas = Money::from_cents(d.pagas.importe.cents() * i64::from(d.pagas.numero));
    let prorrata = if diaria {
        proporcion(anual_pagas, dias, 365)
    } else {
        proporcion(anual_pagas, dias, 360)
    };
    if d.pagas.prorrateadas && !prorrata.is_zero() {
        devengos.push(Devengo {
            concepto: "Prorrata de pagas extraordinarias".into(),
            importe: prorrata,
            clase: ClaseConcepto::Salarial,
            extraordinaria: true,
            horas_extra: false,
        });
    } else if d.pagas.en_este_mes > 0 {
        devengos.push(Devengo {
            concepto: if d.pagas.en_este_mes == 1 {
                "Gratificación extraordinaria".into()
            } else {
                format!("Gratificaciones extraordinarias ({})", d.pagas.en_este_mes)
            },
            importe: Money::from_cents(d.pagas.importe.cents() * i64::from(d.pagas.en_este_mes)),
            clase: ClaseConcepto::Salarial,
            extraordinaria: true,
            horas_extra: false,
        });
    }
    let total_devengado: Money = devengos.iter().map(|x| x.importe).sum();

    // Bases.
    let remuneracion_mensual: Money = devengos
        .iter()
        .filter(|x| !x.extraordinaria && !x.horas_extra)
        .filter(|x| x.clase != ClaseConcepto::NoSalarialExenta)
        .map(|x| x.importe)
        .sum();
    let (base_minima, base_maxima) = topes(t, d.grupo, d.dias, d.jornada)
        .ok_or_else(|| datos("el grupo de cotización va del 1 al 11"))?;
    let bruta = remuneracion_mensual + prorrata;
    let clamp = |m: Money| m.max(base_minima).min(base_maxima);
    let base_comunes = clamp(bruta);
    let bruta_cp = bruta + d.horas_extra;
    let base_profesionales = clamp(bruta_cp);
    let tope = if bruta < base_minima {
        avisos.push(format!(
            "La remuneración ({bruta}) no llega a la base mínima del grupo {}: se cotiza por \
             {base_minima}",
            d.grupo
        ));
        Some(Tope::Minimo)
    } else if bruta > base_maxima {
        Some(Tope::Maximo)
    } else {
        None
    };

    // Cuotas.
    let tp = &t.tipos;
    let desempleo = match d.contrato {
        Contrato::Indefinido => tp.desempleo_indefinido,
        Contrato::Temporal => tp.desempleo_temporal,
    };
    let at = |r: Rate| Reparto {
        empresa: r,
        trabajador: Rate::ZERO,
    };
    let mut cuotas = vec![
        cuota(ClaveCuota::Comunes, base_comunes, tp.comunes),
        cuota(ClaveCuota::Mei, base_comunes, tp.mei),
        cuota(ClaveCuota::Desempleo, base_profesionales, desempleo),
        cuota(ClaveCuota::Formacion, base_profesionales, tp.formacion),
        cuota(ClaveCuota::Fogasa, base_profesionales, tp.fogasa),
        cuota(ClaveCuota::AtIt, base_profesionales, at(d.at_it)),
        cuota(ClaveCuota::AtIms, base_profesionales, at(d.at_ims)),
    ];
    if !d.horas_extra.is_zero() {
        let r = if d.horas_extra_fuerza_mayor {
            tp.horas_fuerza_mayor
        } else {
            tp.horas_resto
        };
        cuotas.push(cuota(ClaveCuota::HorasExtra, d.horas_extra, r));
    }
    // Solidaridad: la retribución que excede de la base máxima, por tramos (hasta el 10 % de la
    // máxima, del 10 al 50 % y el resto), repartida como las contingencias comunes.
    // `TODO(VERIFY)`: art. 19 bis TRLGSS (reparto y retribución computable).
    let exceso = bruta_cp - base_maxima;
    if exceso > Money::ZERO {
        let t1 = exceso.min(proporcion(base_maxima, 10, 100));
        let t2 = (exceso - t1).min(proporcion(base_maxima, 40, 100));
        let t3 = exceso - t1 - t2;
        let total = t1.apply_rate(tp.solidaridad[0])
            + t2.apply_rate(tp.solidaridad[1])
            + t3.apply_rate(tp.solidaridad[2]);
        let cc = tp.comunes;
        let empresa = proporcion(
            total,
            i64::from(cc.empresa.bp()),
            i64::from(cc.total().bp()),
        );
        cuotas.push(Cuota {
            clave: ClaveCuota::Solidaridad,
            concepto: ClaveCuota::Solidaridad.nombre().into(),
            base: exceso,
            tipo_empresa: Rate::ZERO,
            empresa,
            tipo_trabajador: Rate::ZERO,
            trabajador: total - empresa,
        });
        avisos.push(format!(
            "La retribución supera la base máxima ({base_maxima}): se cotiza por la máxima y se \
             añade la cotización adicional de solidaridad sobre {exceso} (art. 19 bis TRLGSS)"
        ));
    }
    let total_trabajador: Money = cuotas.iter().map(|c| c.trabajador).sum();
    let total_empresa: Money = cuotas.iter().map(|c| c.empresa).sum();

    // IRPF.
    let base_irpf: Money = devengos
        .iter()
        .filter(|x| x.clase != ClaseConcepto::NoSalarialExenta)
        .map(|x| x.importe)
        .sum();
    let Retencion::Manual { tipo: tipo_irpf } = d.retencion;
    let retencion_irpf = base_irpf.apply_rate(tipo_irpf);

    let total_deducciones = total_trabajador + retencion_irpf + d.anticipos + d.otras_deducciones;
    let liquido = total_devengado - total_deducciones;
    if liquido.is_negative() {
        return Err(datos(
            "las deducciones superan lo devengado: revisa anticipos y otras deducciones",
        ));
    }
    if d.contrato == Contrato::Temporal {
        avisos.push(
            "Contrato temporal: si dura menos de 30 días hay una cotización adicional a cargo de \
             la empresa al darlo de baja (art. 151 TRLGSS) que TaxPain no calcula"
                .into(),
        );
    }
    if d.jornada.bp() < 10_000 {
        avisos.push(
            "Jornada parcial: la base mínima se ha reducido en proporción a la jornada; la orden \
             de cotización la fija por horas (TODO(VERIFY) con tu caso)"
                .into(),
        );
    }
    for p in t.pendiente {
        avisos.push(format!("Sin cotejar con el BOE: {p}"));
    }
    Ok(Nomina {
        year: d.year,
        month: d.month,
        grupo: d.grupo,
        devengos,
        total_devengado,
        remuneracion_mensual,
        prorrata_pagas: prorrata,
        base_minima,
        base_maxima,
        base_comunes,
        base_profesionales,
        base_horas_extra: d.horas_extra,
        tope,
        cuotas,
        total_trabajador,
        total_empresa,
        base_irpf,
        tipo_irpf,
        retencion_irpf,
        anticipos: d.anticipos,
        otras_deducciones: d.otras_deducciones,
        total_deducciones,
        liquido,
        coste_empresa: total_devengado + total_empresa,
        norma: t.norma.into(),
        avisos,
    })
}

/// Datos que pide el algoritmo de retenciones de la AEAT (arts. 80 a 89 RIRPF).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatosRetencion {
    pub year: i32,
    /// Retribuciones íntegras previstas del año.
    pub retribucion_anual: Money,
    /// Cotizaciones del trabajador previstas del año.
    pub cotizaciones_anuales: Money,
    /// Situación familiar del art. 88 RIRPF (1, 2 o 3).
    pub situacion_familiar: u8,
    pub contrato_inferior_al_ano: bool,
    pub discapacidad: u8,
    pub hijos_menores_3: u8,
    pub hijos_resto: u8,
    pub hijos_por_entero: bool,
    pub ascendientes: u8,
    pub movilidad_geografica: bool,
    pub ano_nacimiento: u16,
}

/// Algoritmo oficial de cálculo del tipo de retención de un año. No hay ninguno implementado:
/// sólo se dará de alta uno cuando su resultado se haya cotejado con el programa de la AEAT
/// ([`PROGRAMA_RETENCIONES`]) en casos de prueba.
pub trait AlgoritmoRetencion: std::fmt::Debug + Sync {
    fn year(&self) -> i32;
    fn tipo(&self, datos: &DatosRetencion) -> Rate;
}

/// Algoritmo verificado del año, si lo hay (hoy, ninguno).
pub fn algoritmo_oficial(_year: i32) -> Option<&'static dyn AlgoritmoRetencion> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    fn base() -> DatosNomina {
        DatosNomina {
            year: 2026,
            month: 10,
            grupo: 5,
            contrato: Contrato::Indefinido,
            dias: 30,
            jornada: Rate::percent(100),
            salario_base: eur(1800),
            complementos: vec![
                Concepto {
                    nombre: "Plus de convenio".into(),
                    importe: eur(200),
                    clase: ClaseConcepto::Salarial,
                },
                Concepto {
                    nombre: "Dietas".into(),
                    importe: eur(100),
                    clase: ClaseConcepto::NoSalarialExenta,
                },
            ],
            horas_extra: Money::ZERO,
            horas_extra_fuerza_mayor: false,
            pagas: PagasExtra {
                numero: 2,
                importe: eur(1800),
                prorrateadas: false,
                en_este_mes: 0,
            },
            at_it: Rate::from_bp(65),
            at_ims: Rate::from_bp(35),
            retencion: Retencion::Manual {
                tipo: Rate::percent(12),
            },
            anticipos: Money::ZERO,
            otras_deducciones: Money::ZERO,
        }
    }

    fn c(n: &Nomina, k: ClaveCuota) -> &Cuota {
        n.cuotas.iter().find(|c| c.clave == k).expect("cuota")
    }

    #[test]
    fn nomina_general_con_prorrata_en_la_base() {
        let n = calcular(&base()).unwrap();
        // 1.800 + 200 + 100 de dietas.
        assert_eq!(n.total_devengado, eur(2100));
        assert_eq!(n.remuneracion_mensual, eur(2000));
        // 2 × 1.800 / 12 = 300.
        assert_eq!(n.prorrata_pagas, eur(300));
        assert_eq!(n.base_comunes, eur(2300));
        assert_eq!(n.base_profesionales, eur(2300));
        assert_eq!(n.tope, None);
        // 2.300 × 4,70 % = 108,10; × 23,60 % = 542,80.
        let cc = c(&n, ClaveCuota::Comunes);
        assert_eq!(
            (cc.trabajador, cc.empresa),
            (
                eur(108) + Money::from_cents(10),
                eur(542) + Money::from_cents(80)
            )
        );
        // MEI 2026: 0,15 % y 0,75 %.
        let mei = c(&n, ClaveCuota::Mei);
        assert_eq!(
            (mei.trabajador, mei.empresa),
            (Money::from_cents(345), Money::from_cents(1725))
        );
        let des = c(&n, ClaveCuota::Desempleo);
        assert_eq!(
            (des.trabajador, des.empresa),
            (Money::from_cents(3565), Money::from_cents(12650))
        );
        assert_eq!(
            c(&n, ClaveCuota::Formacion).trabajador,
            Money::from_cents(230)
        );
        assert_eq!(c(&n, ClaveCuota::Fogasa).empresa, Money::from_cents(460));
        assert_eq!(c(&n, ClaveCuota::AtIt).empresa, Money::from_cents(1495));
        assert_eq!(c(&n, ClaveCuota::AtIms).trabajador, Money::ZERO);
        // Trabajador: 108,10 + 3,45 + 35,65 + 2,30 = 149,50.
        assert_eq!(n.total_trabajador, Money::from_cents(14950));
        // Empresa: 542,80 + 17,25 + 126,50 + 13,80 + 4,60 + 14,95 + 8,05 = 727,95.
        assert_eq!(n.total_empresa, Money::from_cents(72795));
        // IRPF sobre 2.000 (sin las dietas exentas): 240.
        assert_eq!(n.base_irpf, eur(2000));
        assert_eq!(n.retencion_irpf, eur(240));
        assert_eq!(n.liquido, eur(2100) - Money::from_cents(14950) - eur(240));
        assert_eq!(n.coste_empresa, eur(2100) + Money::from_cents(72795));
    }

    #[test]
    fn paga_extra_del_mes_no_entra_en_la_base() {
        let mut d = base();
        d.pagas.en_este_mes = 1;
        let n = calcular(&d).unwrap();
        assert_eq!(n.total_devengado, eur(3900));
        assert_eq!(n.base_comunes, eur(2300));
        assert_eq!(n.base_irpf, eur(3800));
        let mut p = base();
        p.pagas.prorrateadas = true;
        let n = calcular(&p).unwrap();
        assert_eq!(n.total_devengado, eur(2400));
        assert_eq!(n.base_comunes, eur(2300));
        p.pagas.en_este_mes = 1;
        assert!(calcular(&p).is_err());
    }

    #[test]
    fn topes_minimo_y_maximo() {
        let mut d = base();
        d.salario_base = eur(1000);
        d.complementos.clear();
        d.pagas.numero = 0;
        let n = calcular(&d).unwrap();
        assert_eq!(n.tope, Some(Tope::Minimo));
        assert_eq!(n.base_comunes, Money::from_cents(142_440));
        // Media jornada: la mínima a la mitad.
        d.jornada = Rate::percent(50);
        let n = calcular(&d).unwrap();
        assert_eq!(n.base_comunes, eur(1000));
        // Por encima de la máxima: tope y solidaridad.
        let mut d = base();
        d.salario_base = eur(6000);
        d.complementos.clear();
        d.pagas.numero = 0;
        let n = calcular(&d).unwrap();
        assert_eq!(n.tope, Some(Tope::Maximo));
        assert_eq!(n.base_comunes, Money::from_cents(510_120));
        let s = c(&n, ClaveCuota::Solidaridad);
        // Exceso 898,80: 510,12 al 1,15 % (5,87) y 388,68 al 1,25 % (4,86) = 10,73.
        assert_eq!(s.base, Money::from_cents(89_880));
        assert_eq!(s.empresa + s.trabajador, Money::from_cents(1073));
        assert_eq!(s.empresa, Money::from_cents(895));
    }

    #[test]
    fn horas_extra_y_contrato_temporal() {
        let mut d = base();
        d.horas_extra = eur(100);
        d.contrato = Contrato::Temporal;
        let n = calcular(&d).unwrap();
        assert_eq!(n.base_comunes, eur(2300));
        assert_eq!(n.base_profesionales, eur(2400));
        let h = c(&n, ClaveCuota::HorasExtra);
        assert_eq!(
            (h.trabajador, h.empresa),
            (Money::from_cents(470), Money::from_cents(2360))
        );
        let des = c(&n, ClaveCuota::Desempleo);
        assert_eq!(
            (des.tipo_trabajador, des.tipo_empresa),
            (Rate::from_bp(160), Rate::from_bp(670))
        );
        assert!(n.avisos.iter().any(|a| a.contains("art. 151")));
    }

    #[test]
    fn grupo_de_base_diaria_y_dias_sueltos() {
        let mut d = base();
        d.grupo = 9;
        d.dias = 31;
        d.salario_base = eur(1200);
        d.complementos.clear();
        d.pagas.numero = 0;
        let n = calcular(&d).unwrap();
        // 47,48 × 31 = 1.471,88.
        assert_eq!(n.base_minima, Money::from_cents(147_188));
        assert_eq!(n.base_comunes, Money::from_cents(147_188));
        let mut d = base();
        d.dias = 15;
        let (min, max) = topes(&tablas::TABLA_2026, 5, 15, Rate::percent(100)).unwrap();
        assert_eq!(min, Money::from_cents(71_220));
        assert_eq!(max, Money::from_cents(255_060));
        d.dias = 31;
        assert!(calcular(&d).is_err());
    }

    #[test]
    fn sin_tabla_y_datos_malos() {
        let mut d = base();
        d.year = 2027;
        assert_eq!(calcular(&d), Err(NominaError::SinTabla(2027)));
        let mut d = base();
        d.grupo = 12;
        assert!(calcular(&d).is_err());
        let mut d = base();
        d.anticipos = eur(5000);
        assert!(calcular(&d).is_err());
        assert!(algoritmo_oficial(2026).is_none());
    }

    #[test]
    fn proporcion_redondea() {
        assert_eq!(
            proporcion(Money::from_cents(100), 1, 3),
            Money::from_cents(33)
        );
        assert_eq!(
            proporcion(Money::from_cents(100), 2, 3),
            Money::from_cents(67)
        );
        assert_eq!(
            proporcion(Money::from_cents(-100), 2, 3),
            Money::from_cents(-67)
        );
        assert_eq!(proporcion(Money::from_cents(100), 1, 0), Money::ZERO);
    }
}
