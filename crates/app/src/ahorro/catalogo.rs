//! Las fichas de los incentivos: texto fijo (qué es, requisitos, pasos, riesgos y norma) y la
//! función que evalúa si le encaja a la empresa.
//!
//! Las cifras legales son las de 2026, contrastadas el 2026-10-30 con los textos consolidados del
//! BOE (LIS a 02-09-2026; LIRPF, RIRPF y LIVA a 07-10-2026; Ley 49/2002), el diseño del modelo
//! 200 de 2025 (las casillas que se citan son las de ese diseño: las de 2026 aún no están
//! publicadas), los manuales prácticos de Sociedades y Renta 2025 y las consultas de la DGT que
//! cita cada ficha. Dudas sin criterio publicado (se dicen en la propia ficha):
//! - TODO(VERIFY): si las empresas de reducida dimensión de 1 a 10 M€ (23 % en 2026) pueden
//!   aplicar la reserva de nivelación: el art. 105 LIS exige el tipo «del primer párrafo del art.
//!   29.1» y la Ley 7/2024 dejó el suyo en otro párrafo (el de las microempresas sí está en el
//!   primero); la DGT la negó al 15 % de nueva creación (V3495-19), pero la admitió a las ERD con
//!   la escala transitoria de la DT 34.ª j) (V4068-16). Para ellas sale como «puede interesarte».
//! - TODO(VERIFY): cómo contabiliza y trata en el IS el financiador del art. 39.7 su aportación
//!   (si es gasto o no): la ganancia que se estima es la de la deducción (hasta el 20 % de lo
//!   aportado), sin ese efecto.
//! - TODO(VERIFY): si a las criptomonedas se les aplica la regla de recompra en un año del art.
//!   33.5.e LIRPF (la de los dos meses de las acciones no, porque no son valores: DGT V0525-25).

use taxpain_core::money::{Money, Rate};
use time::{Date, Month};

use super::{Area, Ctx, Encaje, Para, tipo_ahorro};

/// Resultado de evaluar una ficha con los datos de la empresa.
pub(crate) struct Evaluacion {
    pub encaje: Encaje,
    pub motivo: String,
    pub ahorro: Option<Money>,
    pub calculo: Option<String>,
    pub plazo: Option<Date>,
    /// Lo que rebaja la base imponible si se sigue (para calcular las que dependen de la cuota).
    pub rebaja: Option<Money>,
    /// Reserva de capitalización: la rebaja antes del tope del 20 % (o 25 %) de la base, que se
    /// recalcula cuando otras rebajas bajan la base.
    pub sin_tope: Option<Money>,
}

fn ev(encaje: Encaje, motivo: impl Into<String>) -> Evaluacion {
    Evaluacion {
        encaje,
        motivo: motivo.into(),
        ahorro: None,
        calculo: None,
        plazo: None,
        rebaja: None,
        sin_tope: None,
    }
}

impl Evaluacion {
    fn ahorro(mut self, a: Option<Money>, calculo: impl Into<String>) -> Self {
        self.ahorro = a;
        if a.is_some() {
            self.calculo = Some(calculo.into());
        }
        self
    }

    fn plazo(mut self, d: Date) -> Self {
        self.plazo = Some(d);
        self
    }

    fn rebaja(mut self, m: Money) -> Self {
        self.rebaja = Some(m);
        self
    }

    fn sin_tope(mut self, m: Money) -> Self {
        self.sin_tope = Some(m);
        self
    }
}

/// Lo que rebajan la base las recomendaciones que convienen (capitalización, exención de
/// dividendos y deterioro de créditos): la cuota que queda es la que limita las deducciones.
fn rebajas_que_convienen(c: &Ctx) -> Money {
    let conviene = |e: Evaluacion| (e.encaje == Encaje::Conviene).then_some(e);
    // Exención y deterioro rebajan la base previa; la capitalización tiene su tope sobre esa
    // base ya rebajada (art. 25.1 LIS), así que se recalcula.
    let previas: Money = [exencion_participadas(c), deterioro_creditos(c)]
        .into_iter()
        .filter_map(conviene)
        .filter_map(|e| e.rebaja)
        .sum();
    let capitalizacion = conviene(capitalizacion(c))
        .and_then(|e| e.sin_tope.or(e.rebaja))
        .unwrap_or(Money::ZERO);
    let base = c.base.unwrap_or(Money::ZERO).max(Money::ZERO);
    let tope = pct(
        (base - previas).max(Money::ZERO),
        if c.incn.is_some_and(|x| x < Money::from_euros(1_000_000, 0)) {
            25
        } else {
            20
        },
    );
    previas + capitalizacion.min(tope)
}

#[derive(Debug)]
pub struct Ficha {
    pub id: &'static str,
    pub para: Para,
    pub area: Area,
    pub titulo: &'static str,
    pub resumen: &'static str,
    pub requisitos: &'static [&'static str],
    pub pasos: &'static [&'static str],
    pub riesgos: &'static [&'static str],
    pub norma: &'static str,
    pub enlace: &'static str,
    pub pantalla: Option<&'static str>,
    /// El ahorro es impuesto aplazado, no ahorrado.
    pub aplazado: bool,
    pub(crate) evaluar: fn(&Ctx) -> Evaluacion,
}

fn pct(m: Money, p: i32) -> Money {
    m.apply_rate(Rate::percent(p))
}

/// Más de la mitad del activo (media de los balances trimestrales, con las partidas marcadas) son
/// valores o bienes no afectos: sociedad patrimonial (sin las ventajas de las empresas pequeñas).
fn patrimonial(c: &Ctx) -> bool {
    c.inversiones.is_some_and(|(_, otras)| otras > 50)
}

/// Aviso para las ventajas de las empresas pequeñas cuando más de la mitad del activo son valores
/// o bienes no afectos (sociedad patrimonial, art. 5.2 LIS).
fn aviso_patrimonial(c: &Ctx) -> String {
    match c.inversiones {
        Some((_, otras)) if otras > 50 => format!(
            " Ojo: en la media de los balances trimestrales el {otras} % del activo son valores \
             o bienes no afectos; si la sociedad es patrimonial, tributa al 25 % y no tiene esta \
             ventaja."
        ),
        _ => String::new(),
    }
}

/// Base positiva del ejercicio (o `Err` con la evaluación que toca si no la hay).
fn base_positiva(c: &Ctx) -> Result<Money, Evaluacion> {
    match c.base {
        Some(b) if b.cents() > 0 => Ok(b),
        Some(_) => Err(ev(
            Encaje::NoAplica,
            "Con las cifras de este año no hay beneficio, así que no hay impuesto que rebajar.",
        )),
        None => Err(ev(
            Encaje::Valorar,
            "TaxPain no tiene aún el beneficio de este año: con la contabilidad o el último \
             Impuesto sobre Sociedades se calcularía cuánto te ahorras.",
        )),
    }
}

// ---------------------------------------------------------------- Sociedades: beneficio

/// Reserva de capitalización: % de reducción del incremento de fondos propios (art. 25.1 LIS,
/// redacción de la Ley 7/2024, desde 2025; sube al 23 %, 26,5 % o 30 % si la plantilla media
/// crece un 2 %, un 5 % o un 10 %).
const CAPITALIZACION_PCT: i32 = 20;

fn capitalizacion(c: &Ctx) -> Evaluacion {
    let base = match base_positiva(c) {
        Ok(b) => b,
        Err(e) => return e,
    };
    if c.capitalizacion_indicada.cents() > 0 {
        return ev(
            Encaje::Aprovechado,
            format!(
                "Ya tienes indicada una reserva de capitalización de {} para {} en «Datos de los \
                 modelos».",
                c.capitalizacion_indicada, c.año
            ),
        );
    }
    // Tope: 20 % de la base previa (25 % si la cifra de negocios del año anterior es < 1 M€).
    let tope_pct = if c.incn.is_some_and(|x| x < Money::from_euros(1_000_000, 0)) {
        25
    } else {
        20
    };
    let antes = c
        .capitalizacion_anterior
        .map(|(y, m)| format!(" En {y} ya la aplicaste ({m})."))
        .unwrap_or_default();
    let plantilla = " Si además aumentas la plantilla media (un 2 % o más) y la mantienes, el \
                     porcentaje sube hasta el 30 %.";
    // Si la junta ya ha aplicado el resultado, el aumento es el que dice el diario.
    if let Some(aumento) = c.aumento_contabilizado {
        // Salen reservas y en los tres años anteriores se aplicó la reserva: puede perderse.
        if aumento.cents() < 0
            && let Some((y, m)) = c.capitalizacion_anterior
            && y >= c.año - 3
        {
            return ev(
                Encaje::Valorar,
                format!(
                    "Este año salen {} de reservas: si los fondos propios bajan del aumento que \
                     justificó la reserva de capitalización de {y} ({m}), hay que devolver ese \
                     ahorro con intereses (art. 25.1.a de la Ley 27/2014). Revísalo con tu asesor \
                     antes de repartir.",
                    -aumento
                ),
            );
        }
        if aumento.cents() <= 0 {
            return ev(
                Encaje::Valorar,
                format!(
                    "Este año el resultado de {} no ha ido a reservas voluntarias, remanente ni a \
                     compensar pérdidas, así que los fondos propios que cuentan no crecen.{antes}",
                    c.año - 1
                ),
            );
        }
        let reduccion = pct(aumento, CAPITALIZACION_PCT).min(pct(base, tope_pct));
        let sin_tope = pct(aumento, CAPITALIZACION_PCT);
        return ev(
            Encaje::Conviene,
            format!(
                "Este año han ido {aumento} a reservas (voluntarias, remanente o compensar \
                 pérdidas): si se mantienen hasta el cierre de {} sin repartirlas, puedes rebajar \
                 la base en {reduccion}.{antes}{plantilla}",
                c.año
            ),
        )
        .rebaja(reduccion)
        .sin_tope(sin_tope)
        .ahorro(
            c.ahorro_base(reduccion),
            format!(
                "{CAPITALIZACION_PCT} % de {aumento} (como mucho el {tope_pct} % de la base, \
                 {base}), por tu tipo del Impuesto sobre Sociedades"
            ),
        )
        .plazo(c.cierre);
    }
    match &c.resultado_anterior {
        Some((r, fuente)) if r.cents() > 0 => {
            let legal = c.reserva_legal.unwrap_or(Money::ZERO);
            let incremento = (*r - c.dividendos_acordados - legal).max(Money::ZERO);
            if incremento.is_zero() {
                return ev(
                    Encaje::Valorar,
                    format!(
                        "El beneficio de {} ({r}) se ha repartido este año o va a la reserva legal, \
                         así que los fondos propios no crecen. Si dejas parte en reservas, puedes \
                         rebajar la base un {CAPITALIZACION_PCT} % de lo que dejes.{antes}",
                        c.año - 1
                    ),
                );
            }
            let reduccion = pct(incremento, CAPITALIZACION_PCT).min(pct(base, tope_pct));
            let sin_tope = pct(incremento, CAPITALIZACION_PCT);
            let mut menos = Vec::new();
            if c.dividendos_acordados.cents() > 0 {
                menos.push(format!("{} de dividendos", c.dividendos_acordados));
            }
            if legal.cents() > 0 {
                menos.push(format!("{legal} que van a la reserva legal (no cuentan)"));
            }
            let menos = if menos.is_empty() {
                String::new()
            } else {
                format!(", menos {}", menos.join(" y "))
            };
            let sin_legal = if c.reserva_legal.is_none() {
                " (sin contar lo que tenga que ir a la reserva legal, que no cuenta)"
            } else {
                ""
            };
            ev(
                Encaje::Conviene,
                format!(
                    "En {} ganaste {r} ({fuente}){menos}. Si dejas {incremento} en reservas hasta \
                     el cierre de {} (sin repartirlo){sin_legal}, puedes rebajar la base en \
                     {reduccion}.{antes}{plantilla}",
                    c.año - 1,
                    c.año
                ),
            )
            .rebaja(reduccion)
            .sin_tope(sin_tope)
            .ahorro(
                c.ahorro_base(reduccion),
                format!(
                    "{CAPITALIZACION_PCT} % de {incremento} (como mucho el {tope_pct} % de la base, \
                     {base}), por tu tipo del Impuesto sobre Sociedades"
                ),
            )
            .plazo(c.cierre)
        }
        _ => ev(
            Encaje::Valorar,
            format!(
                "Si al cierre de {} tus fondos propios han crecido con beneficios que dejas en \
                 reservas (las aportaciones de los socios no cuentan), rebajas la base un \
                 {CAPITALIZACION_PCT} % de ese aumento.{antes}{plantilla}",
                c.año
            ),
        ),
    }
}

fn nivelacion(c: &Ctx) -> Evaluacion {
    if c.regimen == taxpain_models::m200::TaxRegime::NuevaCreacion
        || c.regimen_ajustes == taxpain_models::m200::TaxRegime::NuevaCreacion
    {
        return ev(
            Encaje::NoAplica,
            "Mientras tributes al 15 % de nueva creación no puedes aplicarla: ese tipo no está en \
             el primer párrafo del art. 29.1 (DGT V3495-19).",
        );
    }
    if c.incn.is_none() && !c.fisica {
        return ev(
            Encaje::Valorar,
            format!(
                "Si tu cifra de negocios de {} fue menor de 10 millones, puedes rebajar la base \
                 hasta un 10 % y compensarla con pérdidas de los cinco años siguientes.",
                c.año - 1
            ),
        );
    }
    if !c.erd {
        return ev(
            Encaje::NoAplica,
            "Sólo para empresas de reducida dimensión (cifra de negocios del año anterior menor \
             de 10 millones).",
        );
    }
    let base = match base_positiva(c) {
        Ok(b) => b,
        Err(e) => return e,
    };
    // 10 % de la base después de la reserva de capitalización y de compensar las bases negativas,
    // como mucho 1 M€ (art. 105.1).
    let tras_bin = c.base_liquidable(rebajas_que_convienen(c));
    let reduccion = pct(tras_bin, 10).min(c.millon());
    let _ = base;
    if reduccion.is_zero() {
        return ev(
            Encaje::NoAplica,
            "Las bases negativas de años anteriores absorben el beneficio de este año.",
        );
    }
    let antes = c
        .nivelacion_anterior
        .map(|(y, m)| {
            format!(
                " En {y} ya la aplicaste ({m}): esa rebaja se suma a la base si tienes pérdidas en \
                 los cinco años siguientes o, si no, en el quinto."
            )
        })
        .unwrap_or_default();
    // TODO(VERIFY): las de reducida dimensión de 1 a 10 M€ tributan al 23 % con un tipo que la
    // Ley 7/2024 sacó del primer párrafo del art. 29.1 (el que exige el art. 105); el de las
    // microempresas sí está en él.
    let es_micro = c.regimen == taxpain_models::m200::TaxRegime::Micro;
    let micro = es_micro && !patrimonial(c);
    let duda = if es_micro {
        ""
    } else {
        " Confírmalo antes con tu asesor: la ley la reserva a quien tributa al tipo del primer \
         párrafo del art. 29.1 y desde 2025 el de las empresas de reducida dimensión (23 %) está en \
         otro párrafo. Hacienda no lo ha aclarado: la negó al 15 % de nueva creación (V3495-19), \
         pero en 2016 la admitió con la escala transitoria de las empresas de reducida dimensión \
         (V4068-16), y el modelo 200 la sigue admitiendo."
    };
    ev(
        if micro {
            Encaje::Conviene
        } else {
            Encaje::Valorar
        },
        format!(
            "Eres empresa de reducida dimensión y este año tienes beneficio: puedes rebajar la \
             base en {reduccion} (el 10 %). No es un ahorro definitivo: lo que rebajas ahora se \
             suma a la base si tienes pérdidas en los cinco años siguientes, y si no, en el \
             quinto.{duda}{antes}{}",
            aviso_patrimonial(c)
        ),
    )
    .ahorro(
        c.ahorro_desde(tras_bin, reduccion),
        format!(
            "10 % de la base después de las demás rebajas que te convienen y de las bases \
             negativas ({tras_bin}) por tu tipo; es impuesto aplazado cinco años"
        ),
    )
}

fn tipo_reducido(c: &Ctx) -> Evaluacion {
    use taxpain_models::m200::{TaxRegime, gross_tax};
    if c.regimen == TaxRegime::NuevaCreacion || c.regimen_ajustes == TaxRegime::NuevaCreacion {
        return ev(
            Encaje::Aprovechado,
            "Tienes marcado el tipo de entidad de nueva creación (15 %).",
        );
    }
    if c.incn.is_none() {
        let (a, b, r) = taxpain_models::m200_catalogo::tipos(c.año);
        let año = c.año;
        return ev(
            Encaje::Valorar,
            format!(
                "TaxPain no sabe tu cifra de negocios del año anterior: si es menor de 1 millón, \
                 en {año} tributas al {a} % (primeros 50.000 €) y al {b} % (resto); si es menor \
                 de 10 millones, al {r} %. Comprueba el régimen en «Datos de los modelos»."
            ),
        );
    }
    let que = super::regimen_corto(c.regimen);
    let puesto = super::regimen_corto(c.regimen_ajustes);
    if c.regimen_ajustes == c.regimen {
        return if c.regimen == TaxRegime::General {
            ev(
                Encaje::NoAplica,
                "Con tu cifra de negocios (10 millones o más) tributas al tipo general.",
            )
        } else {
            ev(
                Encaje::Aprovechado,
                format!("«Datos de los modelos» ya tiene el tipo {que}."),
            )
        };
    }
    let liquidable = c.base_liquidable(Money::ZERO);
    let diferencia = if liquidable.cents() > 0 {
        c.cuota_de(liquidable, c.regimen_ajustes) - c.cuota_de(liquidable, c.regimen)
    } else {
        Money::ZERO
    };
    // Los ajustes tienen un tipo más bajo que el que corresponde: el borrador calcula de menos.
    if diferencia.cents() < 0 {
        return ev(
            Encaje::Valorar,
            format!(
                "En «Datos de los modelos» está el tipo {puesto}, más bajo que el que te \
                 corresponde por tu cifra de negocios (el {que}): el borrador del 200 de TaxPain \
                 calcula unos {} de menos. Cámbialo.",
                -diferencia
            ),
        );
    }
    // Si el último 200 presentado ya llevaba un tipo reducido, la gestoría lo aplica: sólo hay
    // que corregir los ajustes de TaxPain para que su borrador no calcule de más.
    if let Some((y, t)) = c.tipo_presentado
        && t.cents() > 0
        && t.cents() < 2_500
    {
        return ev(
            Encaje::Aprovechado,
            format!(
                "El 200 de {y} ya se presentó con un tipo reducido ({},{:02} %). En «Datos de los \
                 modelos» está el {puesto}: pon el {que} para que el borrador de TaxPain no \
                 calcule de más.",
                t.cents() / 100,
                t.cents() % 100
            ),
        );
    }
    let cuanto = if diferencia.cents() > 0 {
        format!(" (unos {diferencia} este año)")
    } else {
        String::new()
    };
    // El tipo reducido se aplica por ley: no es un ahorro que haya que buscar, sólo que el borrador
    // de TaxPain lo tenga. Si el último 200 se presentó al 25 %, puede haber algo que rectificar.
    let presentado = match c.tipo_presentado {
        Some((y, t))
            if t.cents() == 2_500
                && gross_tax(Money::from_euros(100_000, 0), c.regimen, y)
                    .1
                    .bp()
                    < 2_500 =>
        {
            format!(
                " El 200 de {y} se presentó al 25 %: si ese año también te correspondía el tipo \
                 reducido, pide la rectificación."
            )
        }
        _ => String::new(),
    };
    ev(
        Encaje::Valorar,
        format!(
            "En «Datos de los modelos» está el tipo {puesto}, pero por tu cifra de negocios te \
             corresponde el {que}: el borrador del 200 de TaxPain calcula de más{cuanto}. \
             Cámbialo, y si el 200 te lo presenta la gestoría, comprueba que aplica el tipo \
             reducido.{presentado}{}",
            aviso_patrimonial(c)
        ),
    )
}

fn bases_negativas(c: &Ctx) -> Evaluacion {
    if c.bin_indicadas.cents() > 0 {
        return ev(
            Encaje::Aprovechado,
            format!(
                "Tienes indicadas {} de bases negativas pendientes en «Datos de los modelos»: el \
                 borrador del 200 las compensa (hasta 1 millón o el 70 % de la base).",
                c.bin_indicadas
            ),
        );
    }
    let Some((y, pendiente)) = c.bin_arrastre.filter(|(_, m)| m.cents() > 0) else {
        // Sin 200 del año anterior pero con pérdidas en su contabilidad.
        if c.bin_arrastre.is_none_or(|(y, _)| y < c.año - 1)
            && let Some((r, fuente)) = &c.resultado_anterior
            && r.cents() < 0
        {
            return ev(
                Encaje::Valorar,
                format!(
                    "En {} hubo pérdidas ({}, {fuente}) y TaxPain no tiene su Impuesto sobre \
                     Sociedades: si dieron base imponible negativa, indícala en «Datos de los \
                     modelos» para compensarla.",
                    c.año - 1,
                    -*r
                ),
            );
        }
        if c.bin_arrastre.is_some_and(|(y, _)| y == c.año - 1) {
            return ev(
                Encaje::NoAplica,
                "Según los Impuestos sobre Sociedades presentados, no quedan pérdidas de años \
                 anteriores por compensar.",
            );
        }
        return ev(
            Encaje::Valorar,
            "TaxPain no tiene tus Impuestos sobre Sociedades anteriores (o falta alguno): importa \
             el último 200 o indica en «Datos de los modelos» las bases negativas pendientes; si \
             las hay, se compensan con el beneficio de este año.",
        );
    };
    let liquidable = c.base_liquidable(Money::ZERO);
    let ahorro = match c.base.filter(|b| b.cents() > 0) {
        Some(b) => {
            let sin = c.cuota_de(b, c.regimen);
            let con = c.cuota_de(liquidable, c.regimen);
            Some(sin - con).filter(|a| a.cents() > 0)
        }
        None => None,
    };
    ev(
        Encaje::Valorar,
        format!(
            "Según los 200 presentados hasta {y}, quedan unos {pendiente} de bases negativas por \
             compensar y en «Datos de los modelos» no hay ninguna: indícalas para que el borrador \
             del 200 las reste. Si te presenta el 200 la gestoría, comprueba que las compensa."
        ),
    )
    .ahorro(
        ahorro,
        "cuota sin compensar menos cuota compensando (hasta 1 millón o el 70 % de la base)",
    )
}

fn nueva_creacion(c: &Ctx) -> Evaluacion {
    if c.regimen == taxpain_models::m200::TaxRegime::NuevaCreacion {
        return ev(
            Encaje::Aprovechado,
            "Ya tienes marcado el tipo de entidad de nueva creación (15 %) en «Datos de los \
             modelos».",
        );
    }
    // El 15 % es para el primer periodo con base positiva y el siguiente, no los dos primeros
    // años de vida: una sociedad que empezó con pérdidas lo tiene cuando gana por primera vez.
    match c.primer_beneficio {
        Some(Some(y)) if y < c.año - 1 => ev(
            Encaje::NoAplica,
            format!(
                "Ya tuviste beneficio en {y}: el 15 % es para los dos primeros años con beneficio."
            ),
        ),
        Some(_) | None if c.primer_año.is_some_and(|y| y < c.año - 10) => ev(
            Encaje::NoAplica,
            "Sólo los dos primeros años con beneficio de una sociedad nueva.",
        ),
        Some(Some(y)) => ev(
            Encaje::Valorar,
            format!(
                "Tu primer año con beneficio fue {y}: si la sociedad es nueva y la actividad no la \
                 hacía antes un socio o una empresa vinculada, este año también tributa al 15 \
                 %.{}",
                aviso_patrimonial(c)
            ),
        ),
        _ => match c.base {
            Some(b) if b.cents() > 0 => ev(
                Encaje::Valorar,
                "Es tu primer año con beneficio: si la sociedad es nueva y la actividad no la \
                 hacía antes un socio o una empresa vinculada, tributa al 15 % este año y el \
                 siguiente.",
            ),
            _ => ev(
                Encaje::NoAplica,
                "El 15 % empieza el primer año con beneficio.",
            ),
        },
    }
}

fn exencion_participadas(c: &Ctx) -> Evaluacion {
    let d = c.dividendos_participadas;
    if d.cents() <= 0 {
        return ev(
            Encaje::NoAplica,
            "No hay dividendos de participadas (cuenta 760) este año.",
        );
    }
    let exento_todo = pct(d, 95);
    if c.correcciones_disminucion >= exento_todo {
        return ev(
            Encaje::Aprovechado,
            format!(
                "Tienes {d} de dividendos de participadas y una corrección en disminución que \
                 cubre el 95 % ({exento_todo})."
            ),
        );
    }
    // Lo que con seguridad viene de participaciones de peso: empresas del grupo, asociadas o
    // vinculadas (7600-7602) y, si un 200 anterior ya aplicó la exención, la 760 sin subcuenta.
    // Los de otras empresas (7603: acciones de una cartera, fondos, ETF) normalmente no la tienen.
    let generica = (d - c.dividendos_grupo - c.dividendos_otras).max(Money::ZERO);
    let seguro = c.dividendos_grupo
        + if c.exencion_anterior {
            generica
        } else {
            Money::ZERO
        };
    let dudoso = d - seguro;
    let aviso = if dudoso.cents() > 0 {
        format!(
            " De los otros {dudoso}, sólo si vienen de sociedades en las que tienes al menos el 5 % \
             desde hace un año: los de fondos, ETF o acciones de una cartera no están exentos."
        )
    } else {
        String::new()
    };
    if seguro.cents() <= 0 {
        return ev(
            Encaje::Valorar,
            format!(
                "Este año has cobrado {d} de dividendos (cuenta 760). Si vienen de sociedades en \
                 las que tienes al menos el 5 % desde hace un año, el 95 % no tributa; los de \
                 fondos, ETF o acciones de una cartera, sí."
            ),
        );
    }
    let exento = pct(seguro, 95);
    ev(
        Encaje::Conviene,
        format!(
            "Este año has cobrado {seguro} de dividendos de sociedades participadas y no hay \
             corrección en disminución que los deje exentos: si tienes al menos el 5 % desde hace \
             un año, el 95 % ({exento}) no tributa.{aviso}"
        ),
    )
    .rebaja(exento)
    .ahorro(
        c.ahorro_base(exento),
        format!("95 % de {seguro} por tu tipo del Impuesto sobre Sociedades"),
    )
}

fn rectificar(c: &Ctx) -> Evaluacion {
    if c.recuperable.is_empty() {
        return ev(
            Encaje::NoAplica,
            "La revisión de lo presentado en la AEAT no ha encontrado impuesto pagado de más que \
             aún se pueda reclamar (Evolución → Qué revisar).",
        );
    }
    let total: Money = c.recuperable.iter().map(|r| r.importe).sum();
    let lista = c
        .recuperable
        .iter()
        .map(|r| match r.ejercicio {
            Some(y) if !r.titulo.contains(&y.to_string()) => {
                format!("{y}: {} ({})", r.titulo, r.importe)
            }
            _ => format!("{} ({})", r.titulo, r.importe),
        })
        .collect::<Vec<_>>()
        .join("; ");
    let mut e = ev(
        Encaje::Conviene,
        format!(
            "La revisión de lo presentado en la AEAT estima que pagaste de más en años anteriores: \
             {lista}. Se puede pedir la devolución, con intereses."
        ),
    )
    .ahorro(Some(total), "suma de lo que la revisión estima recuperable");
    e.plazo = c.recuperable.iter().filter_map(|r| r.hasta).min();
    e
}

// ---------------------------------------------------------------- Inversiones con deducción

/// Tope de la deducción del financiador sobre lo que aporta (art. 39.7 LIS), calculado sobre el
/// total aportado (DGT V2535-23).
const AUDIOVISUAL_TOPE: i64 = 120;
/// Límite conjunto de las deducciones sobre la cuota (art. 39.1 LIS) y el elevado que tiene el
/// financiador cuando su deducción por el art. 36 llega al 25 % de la cuota (art. 39.7, último
/// párrafo).
const LIMITE_DEDUCCIONES: i32 = 25;
const LIMITE_FINANCIADOR: i32 = 50;

fn audiovisual(c: &Ctx) -> Evaluacion {
    let productora = if c.audiovisual() {
        " Si tu empresa produce, también puede aplicar la deducción por sus propias producciones \
         (30 % del primer millón y 25 % del resto; 20 % en espectáculos en vivo), pero no \
         financiar producciones de empresas vinculadas."
    } else {
        ""
    };
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos (estimación objetiva) no se aplica: es para estimación directa y \
             sociedades.",
        );
    }
    if c.fisica {
        return ev(
            Encaje::Valorar,
            format!(
                "Como autónomo con actividad también puedes financiar una película, serie o \
                 espectáculo y aplicar la deducción en la Renta (casilla [1722], o [1723] en \
                 espectáculos), con el límite del 25 % de tu cuota (50 % si la deducción llega al \
                 25 %). Recuperas lo aportado y ganas hasta un 20 %.{productora}"
            ),
        );
    }
    let Some(cuota) = c.cuota.filter(|q| q.cents() > 0) else {
        return match c.base {
            Some(b) if b.cents() <= 0 => ev(
                Encaje::NoAplica,
                "Sólo sirve si tienes impuesto que pagar: con las cifras de este año no hay \
                 cuota.",
            ),
            _ => ev(
                Encaje::Valorar,
                format!(
                    "Sirve si este año vas a pagar Impuesto sobre Sociedades: TaxPain no tiene aún \
                     la cuota para calcular cuánto podrías aportar.{productora}"
                ),
            ),
        };
    };
    // La cuota que queda después de las demás rebajas que convienen (si no, se aportaría de más y
    // parte de la deducción no cabría este año).
    let rebajas = rebajas_que_convienen(c);
    let cuota = if c.base.is_some_and(|b| b.cents() > 0) {
        c.cuota_de(c.base_liquidable(rebajas), c.regimen)
    } else {
        cuota
    };
    if cuota.cents() <= 0 {
        return ev(
            Encaje::NoAplica,
            "Con las demás rebajas que te convienen no queda cuota del impuesto contra la que \
             aplicar la deducción.",
        );
    }
    let deduccion = pct(cuota, LIMITE_FINANCIADOR);
    let aportacion = Money::from_cents(deduccion.cents() * 100 / AUDIOVISUAL_TOPE);
    let ganancia = deduccion - aportacion;
    let bases_sabidas =
        c.bin_indicadas.cents() > 0 || c.bin_arrastre.is_some_and(|(y, _)| y == c.año - 1);
    let enc = if ganancia >= Money::from_euros(1_000, 0) && bases_sabidas {
        Encaje::Conviene
    } else {
        Encaje::Valorar
    };
    let tras = if rebajas.cents() > 0 {
        format!(" (después de las demás rebajas que te convienen, {rebajas} de base)")
    } else {
        String::new()
    };
    ev(
        enc,
        format!(
            "Con una cuota de unos {cuota}{tras}, aporta como mucho unos {aportacion} a una \
             película, serie o espectáculo español y te desgravas {deduccion} (la mitad de la \
             cuota): recuperas lo aportado y ganas unos {ganancia}. La cuota sale de la \
             contabilidad hasta hoy: recalcúlala antes de firmar. Para usarla en {}, la producción \
             tiene que estar terminada (con la posproducción; en animación, con el certificado de \
             nacionalidad) y con sus certificados antes del cierre, y la comunicación a la AEAT \
             presentada también antes del cierre.{productora}",
            c.año
        ),
    )
    .ahorro(
        Some(ganancia),
        format!(
            "deducción de hasta el {LIMITE_FINANCIADOR} % de la cuota ({deduccion}; el límite \
             general es el {LIMITE_DEDUCCIONES} %, y sube al {LIMITE_FINANCIADOR} % cuando la \
             deducción del financiador llega al {LIMITE_DEDUCCIONES} %) menos lo aportado \
             ({aportacion}, porque la deducción es como mucho el {AUDIOVISUAL_TOPE} % de lo \
             aportado), sin otras deducciones que compartan el límite"
        ),
    )
    .plazo(c.cierre)
}

fn i_mas_d(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos (estimación objetiva) no se aplica: es para estimación directa y \
             sociedades.",
        );
    }
    if c.tech() {
        ev(
            Encaje::Valorar,
            "Tu actividad es tecnológica: si desarrollas software o productos nuevos, o mejoras \
             sustancialmente los que tienes, parte de esos gastos (sueldos del equipo, \
             colaboradores, amortizaciones) puede desgravar un 25 % o más (un 12 % si es \
             innovación tecnológica). Además, los equipos que uses sólo para I+D se pueden \
             amortizar enteros el año de compra.",
        )
    } else {
        ev(
            Encaje::Valorar,
            "Si investigas o desarrollas productos o procesos nuevos (también software), parte de \
             esos gastos desgrava.",
        )
    }
}

fn patent_box(c: &Ctx) -> Evaluacion {
    if c.cnae.is_none() {
        return ev(
            Encaje::Valorar,
            "Si licencias software avanzado registrado, patentes o diseños que has desarrollado \
             tú, hasta el 60 % del beneficio que te dan puede no tributar. (Pon el CNAE en «Datos \
             de los modelos» para que TaxPain lo afine.)",
        );
    }
    if c.tech() || c.industria() {
        ev(
            Encaje::Valorar,
            "Si licencias software avanzado registrado, patentes o diseños que has desarrollado \
             tú, hasta el 60 % del beneficio que te dan puede no tributar.",
        )
    } else {
        ev(
            Encaje::NoAplica,
            "Para quien cede patentes, modelos, diseños o software avanzado registrado propios.",
        )
    }
}

// ---------------------------------------------------------------- Gastos

fn escaso_valor(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos (estimación objetiva) la amortización no cambia lo que pagas.",
        );
    }
    match c.base {
        Some(b) if b.cents() > 0 => ev(
            Encaje::Valorar,
            format!(
                "Si vas a comprar ordenadores, móviles, muebles o herramientas nuevos de hasta 300 \
                 € la unidad, cómpralos antes del cierre ({}): se pueden restar enteros este año \
                 (hasta 25.000 € al año) en lugar de amortizarlos en varios.",
                fecha(c.cierre)
            ),
        )
        .plazo(c.cierre),
        _ => ev(
            Encaje::NoAplica,
            "Sólo adelanta el gasto: sin beneficio este año no te ahorra nada.",
        ),
    }
}

fn amortizacion_acelerada(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos (estimación objetiva) la amortización no cambia lo que pagas.",
        );
    }
    if c.incn.is_none() && !c.fisica {
        return ev(
            Encaje::Valorar,
            format!(
                "Si tu cifra de negocios de {} fue menor de 10 millones y compras maquinaria, \
                 equipos o mobiliario nuevos, puedes amortizarlos el doble de rápido.",
                c.año - 1
            ),
        );
    }
    if !c.erd && !c.fisica {
        return ev(
            Encaje::NoAplica,
            "Sólo para empresas de reducida dimensión y autónomos en estimación directa.",
        );
    }
    match c.base {
        Some(b) if b.cents() > 0 => ev(
            Encaje::Valorar,
            format!(
                "Si compras maquinaria, equipos o mobiliario nuevos, puedes amortizarlos el doble \
                 de rápido: más gasto ahora y menos impuesto este año (se compensa en los \
                 siguientes). Y si además aumentas la plantilla, puedes amortizar libremente hasta \
                 120.000 € por cada persona de aumento.{}",
                aviso_patrimonial(c)
            ),
        ),
        _ => ev(
            Encaje::NoAplica,
            "Sólo adelanta el gasto: sin beneficio este año no te ahorra nada.",
        ),
    }
}

/// Último año con la libertad de amortización de vehículos eléctricos, puntos de recarga y
/// autoconsumo renovable (DA 17.ª y 18.ª LIS, prorrogadas para 2026 por el art. 37 del RDL
/// 7/2026; sin prórroga aprobada para 2027).
const ULTIMO_AÑO_VERDE: i32 = 2026;

fn vehiculos_renovables(c: &Ctx) -> Evaluacion {
    if c.año > ULTIMO_AÑO_VERDE {
        return ev(
            Encaje::NoAplica,
            format!(
                "La libertad de amortización de vehículos eléctricos, puntos de recarga y \
                 autoconsumo renovable era para lo que entró en funcionamiento hasta \
                 {ULTIMO_AÑO_VERDE}; no consta prórroga."
            ),
        );
    }
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos (estimación objetiva) la amortización no cambia lo que pagas.",
        );
    }
    let Some(_) = c.base.filter(|b| b.cents() > 0) else {
        return ev(
            Encaje::NoAplica,
            "Sólo adelanta el gasto: sin beneficio este año no te ahorra nada.",
        );
    };
    if c.fisica {
        // En la Renta un turismo de uso mixto no está afecto a la actividad: no da gasto.
        return ev(
            Encaje::Valorar,
            format!(
                "Como autónomo, un coche sólo es gasto si lo usas exclusivamente para la actividad \
                 (en la práctica, en actividades como el transporte, los taxis, las autoescuelas \
                 o los representantes comerciales). Los puntos de recarga y las placas solares del \
                 local de la actividad sí: si los vas a instalar, que funcionen antes del {} y se \
                 amortizan enteros este año.",
                fecha(c.cierre)
            ),
        )
        .plazo(c.cierre);
    }
    ev(
        Encaje::Valorar,
        format!(
            "Si vas a comprar un coche eléctrico o híbrido enchufable nuevo para la actividad o un \
             punto de recarga, hazlo de forma que esté funcionando antes del cierre ({}); y las \
             placas solares para autoconsumo, antes del {}: se pueden amortizar enteros este \
             año. Si alguien usa el coche también para fines privados, es retribución en \
             especie.",
            fecha(c.cierre),
            fecha(c.cierre.min(fin_año(ULTIMO_AÑO_VERDE)))
        ),
    )
    .plazo(c.cierre)
}

/// Plazo corto de 6 meses para el IVA de las impagadas: volumen de operaciones del año anterior
/// de 6.010.121,04 € o menos (art. 80.Cuatro LIVA). Sin el dato, se supone pequeña.
fn iva_seis_meses(c: &Ctx) -> bool {
    !c.gran_empresa && (c.fisica || c.incn.is_none_or(|x| x <= Money::from_cents(601_012_104)))
}

/// Impagadas cuyo IVA se puede recuperar hoy. Las empresas pequeñas pueden usar el plazo de 6
/// meses o el de un año desde el devengo; las grandes, sólo el de un año. La rectificativa se
/// emite en los 6 meses siguientes al final del plazo, así que la ventana va de los 6 (o 12)
/// meses a los 18 meses desde el devengo (la fecha de la factura, a falta de otra).
fn iva_recuperable(c: &Ctx) -> (Money, usize, Option<Date>, usize) {
    let meses = if iva_seis_meses(c) { 6 } else { 12 };
    let (mut iva, mut n, mut plazo, mut proximas) = (Money::ZERO, 0, None::<Date>, 0);
    for i in &c.impagadas {
        if i.iva.cents() <= 0 {
            continue;
        }
        let desde = sumar_meses(i.emision, meses);
        let hasta = sumar_meses(i.emision, 18);
        if c.hoy >= desde && c.hoy <= hasta {
            iva += i.iva;
            n += 1;
            plazo = Some(plazo.map_or(hasta, |p| p.min(hasta)));
        } else if c.hoy < desde {
            proximas += 1;
        }
    }
    (iva, n, plazo, proximas)
}

fn iva_impagadas(c: &Ctx) -> Evaluacion {
    let (iva, n, plazo, proximas) = iva_recuperable(c);
    let espera = if iva_seis_meses(c) {
        "6 meses"
    } else {
        "un año"
    };
    if n == 0 {
        return if proximas > 0 {
            ev(
                Encaje::Valorar,
                format!(
                    "Tienes {proximas} factura(s) vencidas sin cobrar: si siguen sin cobrarse, \
                     podrás recuperar su IVA cuando pase {espera} desde que se emitieron (y \
                     después de reclamarlas de forma fehaciente)."
                ),
            )
        } else {
            ev(
                Encaje::NoAplica,
                "No hay facturas emitidas impagadas en plazo de recuperar el IVA en la cartera.",
            )
        };
    }
    let mut e = ev(
        Encaje::Conviene,
        format!(
            "Tienes {n} factura(s) emitidas sin cobrar desde hace más de {espera}: si reclamas el \
             cobro de forma fehaciente (burofax, notario o demanda), puedes recuperar el IVA que ya \
             ingresaste ({iva})."
        ),
    )
    .ahorro(
        Some(iva),
        "IVA de la parte pendiente de cobro de esas facturas",
    );
    if let Some(p) = plazo {
        e = e.plazo(p);
    }
    e
}

fn deterioro_creditos(c: &Ctx) -> Evaluacion {
    // Seis meses desde el vencimiento al cierre, de fecha a fecha (art. 13.1.a LIS).
    let limite = restar_meses(c.cierre, 6);
    let vencidas: Vec<_> = c
        .impagadas
        .iter()
        .filter(|i| i.vencimiento <= limite)
        .collect();
    let base: Money = vencidas.iter().map(|i| i.pendiente - i.iva).sum();
    // El 1 % global no incluye los créditos que ya se deterioran uno a uno (art. 104.1 LIS).
    let individuales: Money = vencidas.iter().map(|i| i.pendiente).sum();
    let global = if c.erd && !patrimonial(c) {
        pct((c.clientes - individuales).max(Money::ZERO), 1)
    } else {
        Money::ZERO
    };
    if base.cents() <= 0 && global.cents() <= 0 {
        return ev(
            Encaje::NoAplica,
            "No hay facturas de clientes vencidas hace más de seis meses al cierre.",
        );
    }
    let mut partes = Vec::new();
    if base.cents() > 0 {
        partes.push(format!(
            "{base} de facturas que al cierre llevarán más de seis meses vencidas sin cobrar"
        ));
    }
    if global.cents() > 0 {
        partes.push(format!(
            "el 1 % del saldo de clientes ({global}) como deterioro global por ser empresa de \
             reducida dimensión"
        ));
    }
    let total = base.max(Money::ZERO) + global;
    match c.base {
        Some(b) if b.cents() > 0 => ev(
            Encaje::Conviene,
            format!(
                "Puedes contabilizar como gasto {} y pagar menos impuesto este año (salvo las de \
                 administraciones públicas y empresas vinculadas).",
                partes.join(" y ")
            ),
        )
        .rebaja(total)
        .ahorro(
            c.ahorro_base(total),
            format!("{total} de gasto por tu tipo"),
        )
        .plazo(c.cierre),
        _ => ev(
            Encaje::Valorar,
            format!(
                "Puedes contabilizar como gasto {} (sin beneficio este año, aumenta la base \
                 negativa a compensar).",
                partes.join(" y ")
            ),
        ),
    }
}

// ---------------------------------------------------------------- Personas

fn plan_pensiones_empleo(c: &Ctx) -> Evaluacion {
    if c.trabajadores {
        ev(
            Encaje::Valorar,
            "Tienes trabajadores: la empresa puede aportar a un plan de pensiones de empleo para \
             toda la plantilla. Es gasto para la sociedad (y desgrava un 10 % de lo aportado; si \
             el trabajador cobra más de 27.000 €, en proporción a 27.000 €) y cada persona lo resta \
             de su Renta, hasta 8.500 € al año. El socio que controla la sociedad (cotiza como \
             autónomo) no puede entrar aunque cobre nómina; un administrador del Régimen General \
             sí, en un plan de pensiones de empleo (no en uno de previsión social empresarial).",
        )
    } else {
        ev(
            Encaje::NoAplica,
            "Hace falta tener trabajadores con relación laboral (o administradores del Régimen \
             General): un plan sólo para el socio que controla la sociedad no se admite, porque \
             cotiza como autónomo y no tiene relación laboral aunque cobre nómina (DGT V0618-26).",
        )
    }
}

fn retribucion_flexible(c: &Ctx) -> Evaluacion {
    if c.trabajadores {
        ev(
            Encaje::Valorar,
            "Tienes trabajadores: parte del sueldo puede pagarse en seguro médico, comida, \
             transporte o guardería sin que tribute en su Renta, y sigue siendo gasto para la \
             empresa.",
        )
    } else {
        ev(
            Encaje::NoAplica,
            "Para empresas con trabajadores (también los socios que cobran nómina).",
        )
    }
}

fn discapacidad(c: &Ctx) -> Evaluacion {
    if c.trabajadores {
        ev(
            Encaje::Valorar,
            "Si vas a contratar, cada persona más en plantilla con discapacidad (33 % o más) \
             desgrava 9.000 € (12.000 € si es del 65 % o más).",
        )
    } else {
        ev(
            Encaje::Valorar,
            "Si contratas a una persona con discapacidad (33 % o más), desgrava 9.000 € (12.000 € \
             si es del 65 % o más) por cada persona que aumente la plantilla media respecto del \
             año anterior. Es una vez: si sigue contratada, los años siguientes la plantilla ya no \
             aumenta (si la contratas a mitad de año, se reparte entre este año y el siguiente).",
        )
    }
}

// ---------------------------------------------------------------- Donativos

fn donativos_sociedad(c: &Ctx) -> Evaluacion {
    match c.base {
        Some(b) if b.cents() > 0 => {
            let ejemplo = Money::from_euros(1_000, 0);
            ev(
                Encaje::Valorar,
                format!(
                    "Si la empresa dona a una ONG, fundación o entidad acogida a la Ley de \
                     mecenazgo, desgrava el 40 % (50 % si donaste a esa misma entidad en los dos \
                     años anteriores y no bajas el importe). Por ejemplo, donar {ejemplo} te \
                     cuesta {}.",
                    ejemplo - pct(ejemplo, 40)
                ),
            )
            .plazo(c.cierre)
        }
        _ => ev(
            Encaje::Valorar,
            "Donar a entidades de la Ley de mecenazgo desgrava el 40 % de lo donado (lo que no \
             se pueda aplicar este año, en los diez siguientes).",
        ),
    }
}

fn donativos_autonomo(c: &Ctx) -> Evaluacion {
    ev(
        Encaje::Valorar,
        "En la Renta, los primeros 250 € donados a ONG y fundaciones de la Ley de mecenazgo \
         desgravan el 80 % (donar 250 € te cuesta 50 €) y el resto el 40 % (45 % si donaste a esa \
         misma entidad en los dos años anteriores y no bajas el importe).",
    )
    .plazo(fin_año(c.año))
}

// ---------------------------------------------------------------- Autónomos

fn plan_pensiones_autonomo(c: &Ctx) -> Evaluacion {
    let Some(b) = c.base.filter(|b| b.cents() > 0) else {
        return ev(
            Encaje::Valorar,
            "Lo que aportes a un plan de pensiones rebaja tu Renta: 1.500 € al año en uno \
             individual y hasta 4.250 € más en un plan de empleo simplificado para autónomos.",
        );
    };
    let tributa = c.base_irpf();
    if tributa.is_zero() {
        return ev(
            Encaje::NoAplica,
            format!(
                "Con un rendimiento de unos {b}, apenas pagas Renta: aportar a un plan de \
                 pensiones casi no te ahorraría nada y el dinero queda bloqueado hasta la \
                 jubilación."
            ),
        );
    }
    let marginal = super::marginal_irpf(b);
    let aportacion = Money::from_euros(5_750, 0).min(pct(b, 30)).min(tributa);
    let ahorro = pct(aportacion, i32::from(marginal));
    // Sólo «te conviene» con un tipo alto: con uno bajo, lo que se ahorra ahora puede pagarse
    // igual al rescatarlo, y el dinero queda bloqueado.
    let enc = if marginal >= 30 {
        Encaje::Conviene
    } else {
        Encaje::Valorar
    };
    ev(
        enc,
        format!(
            "Con un rendimiento de unos {b}, aportar {aportacion} a planes de pensiones antes del \
             31 de diciembre (1.500 € en uno individual y el resto en un plan de empleo \
             simplificado para autónomos) rebaja tu Renta de este año."
        ),
    )
    .ahorro(
        Some(ahorro),
        format!(
            "{aportacion} por tu tipo marginal aproximado ({marginal} %); los primeros 5.550 € \
             del rendimiento (el mínimo personal) no tributan; al rescatarlo tributa como \
             rendimiento del trabajo"
        ),
    )
    .plazo(fin_año(c.año))
}

fn inicio_actividad(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "Sólo en estimación directa (no en módulos).",
        );
    }
    if c.inicio_marcado {
        return ev(
            Encaje::Aprovechado,
            "Ya tienes marcada la reducción por inicio de actividad en «Datos de los modelos».",
        );
    }
    // Es el primer periodo con rendimiento positivo y el siguiente (no los dos primeros años).
    match c.primer_beneficio {
        Some(Some(y)) if y < c.año - 1 => ev(
            Encaje::NoAplica,
            format!(
                "Ya tuviste rendimiento positivo en {y}: la reducción es para los dos primeros \
                 años con beneficio."
            ),
        ),
        Some(Some(y)) => ev(
            Encaje::Valorar,
            format!(
                "Tu primer año con beneficio fue {y}: si no tuviste actividad el año anterior al \
                 alta, este año el rendimiento neto también se reduce un 20 %."
            ),
        ),
        _ => match c.base {
            Some(b) if b.cents() > 0 && c.primer_año.is_some_and(|y| y >= c.año - 5) => ev(
                Encaje::Valorar,
                "Parece tu primer año con beneficio: si no tuviste actividad el año anterior al \
                 alta, el rendimiento neto se reduce un 20 % este año y el siguiente.",
            ),
            _ => ev(
                Encaje::NoAplica,
                "Sólo el primer año con rendimiento positivo y el siguiente desde el alta.",
            ),
        },
    }
}

fn gastos_autonomo(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos (estimación objetiva) los gastos no se restan uno a uno.",
        );
    }
    ev(
        Encaje::Valorar,
        "Si trabajas desde casa, parte de luz, agua, gas, teléfono e internet es gasto (30 % de la \
         parte de la vivienda que usas para trabajar). También el seguro médico tuyo y de tu \
         familia (500 € por persona) y las comidas en días de trabajo pagadas con tarjeta (26,67 \
         € al día en España).",
    )
}

fn empresas_nuevas(c: &Ctx) -> Evaluacion {
    ev(
        Encaje::Valorar,
        "Si inviertes en una empresa de nueva o reciente creación (en su constitución o en una \
         ampliación de capital), te desgravas en la parte estatal de la Renta el 50 % de lo \
         invertido, hasta 100.000 € al año.",
    )
    .plazo(fin_año(c.año))
}

// ---------------------------------------------------------------- Criptomonedas

fn cripto_perdidas(c: &Ctx) -> Evaluacion {
    if !c.hay_cripto {
        return ev(
            Encaje::NoAplica,
            "No hay criptomonedas en TaxPain (sección Criptomonedas).",
        );
    }
    let g = c.cripto_ganancias;
    let p = c.cripto_perdidas_latentes;
    if !c.fisica {
        // En el Impuesto sobre Sociedades cualquier pérdida realizada rebaja la base del año (y si
        // sale negativa, se compensa en años siguientes sin plazo).
        if p.cents() > 0 {
            return ev(
                Encaje::Valorar,
                format!(
                    "Tienes posiciones en criptomonedas que, con los últimos precios, pierden {p}. \
                     Si las vendes o cambias antes del cierre ({}), la pérdida rebaja la base de \
                     este año (el deterioro contable de las que están en el inmovilizado \
                     intangible no es deducible; la pérdida al venderlas, sí).",
                    fecha(c.cierre)
                ),
            )
            .ahorro(
                c.ahorro_base(p),
                format!("{p} de pérdida por tu tipo del Impuesto sobre Sociedades"),
            )
            .plazo(c.cierre);
        }
        return ev(
            Encaje::Valorar,
            "Tienes criptomonedas: revisa en Criptomonedas el resultado de cada venta y permuta y \
             el deterioro al cierre.",
        );
    }
    if g.cents() > 0 && p.cents() > 0 {
        let compensa = g.min(p);
        let tipo = tipo_ahorro(g);
        let cuales = c
            .cripto_monedas
            .iter()
            .take(3)
            .map(|(t, m)| format!("{t} (pierde {m})"))
            .collect::<Vec<_>>()
            .join(", ");
        return ev(
            Encaje::Conviene,
            format!(
                "Este año llevas {g} de ganancias con criptomonedas y tienes monedas que, con los \
                 últimos precios, pierden {p}: {cuales}. Si las vendes o cambias antes del 31 de \
                 diciembre, la pérdida compensa la ganancia ({compensa}). Con FIFO se venden \
                 primero las más antiguas: para que salga la pérdida, vende todas las unidades de \
                 esa moneda (si vendes sólo una parte, puede salir ganancia)."
            ),
        )
        .ahorro(
            Some(pct(compensa, i32::from(tipo))),
            format!("{compensa} compensados por un tipo del {tipo} %"),
        )
        .plazo(fin_año(c.año));
    }
    if g.cents() > 0 {
        return ev(
            Encaje::Valorar,
            format!(
                "Llevas {g} de ganancias con criptomonedas este año. Si tienes otras (o acciones y \
                 fondos) en pérdidas, venderlas antes del 31 de diciembre compensa. Pon los precios \
                 de cierre en Criptomonedas para que TaxPain lo calcule."
            ),
        )
        .plazo(fin_año(c.año));
    }
    if g.cents() < 0 {
        return ev(
            Encaje::Valorar,
            format!(
                "Este año llevas {} de pérdidas con criptomonedas: se compensan con otras \
                 ganancias del ahorro (venta de acciones, fondos…) de este año y de los cuatro \
                 siguientes, y hasta un 25 % con intereses y dividendos.",
                -g
            ),
        );
    }
    ev(
        Encaje::Valorar,
        "Tienes criptomonedas: revisa en Criptomonedas la ganancia o pérdida de cada venta y \
         permuta, y si tienes más de 50.000 € en plataformas del extranjero, el modelo 721.",
    )
}

// ---------------------------------------------------------------- Socio-administrador

/// Tramos de la escala general de la Renta (estatal más autonómica estándar) con su tipo
/// marginal: hasta dónde llega cada tipo (como `marginal_irpf`).
const TRAMOS_IRPF: [(i64, u8); 5] = [
    (12_450, 19),
    (20_200, 24),
    (35_200, 30),
    (60_000, 37),
    (300_000, 45),
];

fn retribucion_administrador(c: &Ctx) -> Evaluacion {
    let Some(tipo) = c.tipo() else {
        return ev(
            Encaje::Valorar,
            "Si eres socio y administrador, cobrar una retribución por el cargo es gasto de la \
             sociedad (rebaja el Impuesto sobre Sociedades) y tributa en tu Renta; cobrar \
             dividendos paga primero Sociedades y después la Renta del ahorro. Con beneficio, \
             TaxPain calcula cuál te sale mejor.",
        );
    };
    // El dividendo paga el tipo de Sociedades y, de lo que queda, el 19 % (hasta 6.000 € al
    // año) o el 21 % (de 6.000 € a 50.000 €) en la Renta del ahorro. La retribución sale mejor si
    // el tipo marginal en la Renta es menor. En centésimas de punto: 21 % → 2.100 + 79 × 19.
    let umbral = |ahorro: u32| u32::from(tipo) * 100 + u32::from(100 - tipo) * ahorro;
    let (bajo, alto) = (umbral(19), umbral(21));
    let hasta = TRAMOS_IRPF
        .iter()
        .take_while(|(_, t)| u32::from(*t) * 100 < bajo)
        .last()
        .map(|(e, _)| Money::from_euros(*e, 0));
    let siguiente = TRAMOS_IRPF
        .iter()
        .find(|(_, t)| u32::from(*t) * 100 >= bajo)
        .filter(|(_, t)| u32::from(*t) * 100 < alto)
        .map(|(_, t)| {
            format!(
                "; en el tramo siguiente ({t} %), sólo si tu base del ahorro pasa de 6.000 € al \
                 año (el dividendo tributa entonces al 21 %)"
            )
        })
        .unwrap_or_default();
    let tramo = hasta.map_or(String::new(), |e| {
        format!(
            " En 2026, con una base general de hasta unos {e} en tu Renta (contando la \
             retribución), la retribución sale mejor{siguiente}."
        )
    });
    ev(
        Encaje::Valorar,
        format!(
            "Si eres socio y administrador: cada euro que la sociedad te paga como retribución del \
             cargo es gasto (rebaja el Impuesto sobre Sociedades al {tipo} %) y tributa en tu \
             Renta a tu tipo marginal; cada euro que cobras como dividendo paga antes el {tipo} % \
             de Sociedades y luego el 19 % en la Renta del ahorro (21 % por encima de 6.000 € al \
             año): en total, un {} % (o un {} %). La retribución sale mejor si tu tipo marginal \
             es menor.{tramo} Tiene que estar acordada por la junta y contabilizada dentro del \
             ejercicio (se puede pagar después); si los estatutos no dicen que el cargo es \
             retribuido, cámbialos antes (notario y Registro Mercantil).",
            (bajo + 50) / 100,
            (alto + 50) / 100
        ),
    )
    .plazo(c.cierre)
}

fn empresa_emergente(c: &Ctx) -> Evaluacion {
    if c.regimen == taxpain_models::m200::TaxRegime::NuevaCreacion {
        return ev(
            Encaje::Valorar,
            "Ya tributas al 15 %: si además eres empresa emergente (certificada por ENISA), el 15 \
             % dura cuatro años en lugar de dos y puedes aplazar el impuesto sin intereses.",
        );
    }
    let plazo = if c.industria() { 7 } else { 5 };
    let joven = c.primer_año.is_none_or(|y| y >= c.año - plazo);
    if !joven {
        return ev(
            Encaje::NoAplica,
            "Para empresas innovadoras y escalables certificadas por ENISA con menos de cinco años \
             (siete en biotecnología, energía e industria).",
        );
    }
    if c.dividendos_acordados.cents() > 0 {
        return ev(
            Encaje::NoAplica,
            "Una empresa emergente no puede repartir dividendos, y este año se han acordado.",
        );
    }
    let liquidable = c.base_liquidable(Money::ZERO);
    let ahorro = (liquidable.cents() > 0)
        .then(|| c.cuota_de(liquidable, c.regimen) - pct(liquidable, 15))
        .filter(|a| a.cents() > 0);
    ev(
        Encaje::Valorar,
        format!(
            "{}Si tu modelo de negocio es innovador y escalable (en cualquier sector) y ENISA te \
             certifica como empresa emergente, tributas al 15 % el primer año con beneficio y los \
             tres siguientes, puedes aplazar sin intereses el impuesto de los dos primeros y no \
             haces pagos fraccionados en los siguientes.{}{}",
            if c.tech() {
                "Tu actividad es tecnológica. "
            } else {
                ""
            },
            if c.cnae.is_none() {
                " (Pon el CNAE en «Datos de los modelos» para afinarlo.)"
            } else {
                ""
            },
            aviso_patrimonial(c)
        ),
    )
    .ahorro(
        ahorro,
        "cuota con tu tipo menos el 15 % de la base, cada uno de los cuatro años",
    )
}

fn reinversion_autonomo(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "Sólo en estimación directa (no en módulos).",
        );
    }
    let pct_txt = if c.inicio_marcado { "2,5 %" } else { "5 %" };
    match c.base.filter(|b| b.cents() > 0) {
        Some(b) => ev(
            Encaje::Valorar,
            format!(
                "Si compras maquinaria, equipos u otros elementos nuevos para la actividad (este \
                 año o el que viene), te desgravas en la Renta el {pct_txt} de lo invertido, hasta \
                 el rendimiento neto de la actividad de este año (unos {b})."
            ),
        ),
        None => ev(
            Encaje::NoAplica,
            "Se calcula sobre el rendimiento neto de la actividad: sin beneficio este año no hay \
             deducción.",
        ),
    }
}

// ---------------------------------------------------------------- Otras

/// Último ejercicio con la reserva para inversiones en las Illes Balears (DA 70.ª de la Ley
/// 31/2022: periodos que empiecen de 2023 a 2028).
const ULTIMO_AÑO_RESERVA_BALEARES: i32 = 2028;

fn reserva_baleares(c: &Ctx) -> Evaluacion {
    if c.ccaa != Some(taxpain_core::calendar::Ccaa::IllesBalears) {
        return ev(
            Encaje::NoAplica,
            "Para beneficios de establecimientos en las Illes Balears.",
        );
    }
    if c.año > ULTIMO_AÑO_RESERVA_BALEARES || c.modulos {
        return ev(
            Encaje::NoAplica,
            "Para periodos que empiezan de 2023 a 2028, y en la Renta sólo en estimación directa.",
        );
    }
    ev(
        Encaje::Valorar,
        format!(
            "Si tu actividad está en las Illes Balears y dejas en reservas parte del beneficio de \
             {}, puedes rebajar la base hasta el 90 % de ese beneficio no repartido (en la Renta, \
             una deducción a tu tipo medio) si lo inviertes en los tres años siguientes en activos \
             nuevos usados en las islas, en mejora del medio ambiente o en I+D. Lo que dotes aquí \
             no cuenta para la reserva de capitalización.",
            c.año
        ),
    )
    .plazo(fin_plazo_200(c.cierre))
}

/// Último día del plazo del 200: 25 días naturales después de los seis meses siguientes al
/// cierre (25 de julio con cierre a 31 de diciembre).
fn fin_plazo_200(cierre: Date) -> Date {
    sumar_meses(cierre, 6)
        .checked_add(time::Duration::days(25))
        .unwrap_or(cierre)
}

fn pasar_a_sociedad(c: &Ctx) -> Evaluacion {
    if c.modulos {
        return ev(
            Encaje::NoAplica,
            "En módulos el rendimiento no sale de tus ingresos y gastos: compáralo con tu asesor \
             con tus cifras reales.",
        );
    }
    let marginal = super::marginal_irpf(c.base.unwrap_or(Money::ZERO));
    if marginal < 45 {
        return ev(
            Encaje::NoAplica,
            "Suele compensar sólo con rendimientos altos (tipo marginal del 45 % o más).",
        );
    }
    let b = c.base.unwrap_or(Money::ZERO);
    // Los tipos del año de la Ley 7/2024 (DT 44.ª LIS), no los de un año fijo.
    let (micro1, micro2, _) = taxpain_models::m200_catalogo::tipos(c.año);
    let año = c.año;
    ev(
        Encaje::Valorar,
        format!(
            "Con un rendimiento de unos {b} tributas en la Renta a un tipo marginal del {marginal} \
             %. En una sociedad, el beneficio que se queda dentro paga el Impuesto sobre \
             Sociedades (en {año}, el {micro1} % los primeros 50.000 € y el {micro2} % el resto \
             en las más pequeñas) y lo que saques lo pagas después en tu Renta (como sueldo o dividendos). \
             Puede compensar si no necesitas sacar todo el beneficio cada año."
        ),
    )
}

fn prorrata_especial(c: &Ctx) -> Evaluacion {
    match (c.prorrata, c.prorrata_especial) {
        (_, true) => ev(
            Encaje::Aprovechado,
            "Ya aplicas la prorrata especial del IVA.",
        ),
        (Some(p), false) => ev(
            Encaje::Valorar,
            format!(
                "Aplicas la prorrata general del IVA (el {p} %): con la especial deduces todo el \
                 IVA de lo que usas sólo en operaciones con derecho a deducir (y nada de lo que usas \
                 sólo en las exentas). Compara las dos con los datos del año antes de la última \
                 declaración."
            ),
        )
        .plazo(fin_plazo_4t(c.año)),
        (None, false) => ev(
            Encaje::NoAplica,
            "Sólo para quien hace a la vez operaciones con derecho a deducir el IVA y exentas \
             (prorrata).",
        ),
    }
}

/// Último día para presentar el 303 del cuarto trimestre del año (30 de enero del siguiente).
fn fin_plazo_4t(y: i32) -> Date {
    Date::from_calendar_date(y + 1, Month::January, 30).unwrap_or(Date::MIN)
}

// ---------------------------------------------------------------- Utilidades

fn sumar_meses(d: Date, meses: u8) -> Date {
    let total = d.year() * 12 + i32::from(u8::from(d.month())) - 1 + i32::from(meses);
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let m = Month::try_from(u8::try_from(m).unwrap_or(1)).unwrap_or(Month::January);
    let dias = time::util::days_in_month(m, y);
    Date::from_calendar_date(y, m, d.day().min(dias)).unwrap_or(d)
}

/// `d` menos `meses`, de fecha a fecha (con el último día del mes si no existe: 31-12 → 30-06).
pub(crate) fn restar_meses(d: Date, meses: u8) -> Date {
    let total = d.year() * 12 + i32::from(u8::from(d.month())) - 1 - i32::from(meses);
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let m = Month::try_from(u8::try_from(m).unwrap_or(1)).unwrap_or(Month::January);
    let dias = time::util::days_in_month(m, y);
    Date::from_calendar_date(y, m, d.day().min(dias)).unwrap_or(d)
}

fn fin_año(y: i32) -> Date {
    Date::from_calendar_date(y, Month::December, 31).unwrap_or(Date::MIN)
}

fn fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

// ---------------------------------------------------------------- Fichas

pub static FICHAS: &[Ficha] = &[
    Ficha {
        id: "rectificar",
        para: Para::Ambos,
        area: Area::Beneficio,
        titulo: "Recupera lo que pagaste de más en años anteriores",
        resumen: "Si en una declaración ya presentada se pagó de más (por ejemplo, dividendos de \
                  participadas sin la exención o retenciones sin restar), se puede corregir y la \
                  AEAT devuelve la diferencia con intereses de demora, durante cuatro años desde \
                  el fin del plazo de presentación.",
        requisitos: &[
            "Que no hayan pasado cuatro años desde el fin del plazo para presentar esa \
             declaración.",
            "Tener los documentos que lo justifican (actas de reparto, certificados de \
             retenciones, facturas…).",
        ],
        pasos: &[
            "Abre Evolución → «Qué revisar» y lee el detalle de cada hallazgo con importe.",
            "Pásale el detalle a tu gestoría y pídele que lo confirme y prepare la corrección.",
            "Presenta la autoliquidación rectificativa del modelo en la Sede (o, en ejercicios \
             antiguos, la solicitud de rectificación de la autoliquidación) con los documentos.",
            "Cuando la AEAT devuelva, contabiliza el ingreso (y los intereses de demora como \
             ingreso financiero).",
        ],
        riesgos: &[
            "La AEAT puede revisar todo el ejercicio, no sólo lo que pides.",
            "Pasados los cuatro años, se pierde: mira la fecha límite.",
        ],
        norma: "Arts. 66 y 120.3 de la Ley 58/2003 General Tributaria",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2003-23186#a120",
        pantalla: Some("evolucion"),
        aplazado: false,
        evaluar: rectificar,
    },
    Ficha {
        id: "capitalizacion",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Reserva de capitalización: deja el beneficio en la empresa",
        resumen: "Si los fondos propios de la sociedad crecen durante el año porque dejas en \
                  reservas el beneficio del año anterior (sin repartirlo), la base imponible \
                  baja un 20 % de ese aumento (hasta un 30 % si además crece la plantilla). A \
                  cambio, el aumento se mantiene tres años y se dota una reserva que no se puede \
                  repartir en ese tiempo.",
        requisitos: &[
            "Que los fondos propios al cierre (sin el resultado del año) sean mayores que al \
             empezar (sin el resultado del año anterior).",
            "No cuentan las aportaciones de los socios, las ampliaciones por compensación de \
             créditos, la reserva legal o estatutaria, la reserva de nivelación ni las \
             operaciones con acciones propias: el aumento tiene que salir de beneficios que se \
             quedan en la empresa.",
            "Mantener ese aumento tres años, salvo que haya pérdidas contables.",
            "Dotar una reserva indisponible por el importe de la rebaja, separada en el balance.",
            "La rebaja no puede pasar del 20 % de la base imponible (25 % si la cifra de negocios \
             del año anterior es menor de 1 millón); lo que no quepa se aplica en los dos años \
             siguientes.",
        ],
        pasos: &[
            "Antes del cierre, decide cuánto beneficio del año anterior no vas a repartir como \
             dividendo y no repartas reservas.",
            "Calcula el aumento: fondos propios al cierre sin el resultado del año menos fondos \
             propios al inicio sin el resultado del año anterior, quitando lo que no cuenta.",
            "En el Impuesto sobre Sociedades, pon la rebaja en la casilla [01032] (en TaxPain, \
             «Datos de los modelos» → reserva de capitalización).",
            "En la junta que aprueba las cuentas, dota una reserva separada llamada «Reserva de \
             capitalización» (una subcuenta propia de reservas) con el importe de la rebaja.",
            "Durante tres años, comprueba que los fondos propios no bajan de ese aumento y no \
             repartas esa reserva.",
        ],
        riesgos: &[
            "Si en los tres años siguientes repartes la reserva o bajan los fondos propios, hay \
             que devolver el impuesto ahorrado con intereses.",
            "Si aplicas el porcentaje mayor por aumentar la plantilla, el aumento también hay que \
             mantenerlo tres años.",
        ],
        norma: "Art. 25 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a25",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: capitalizacion,
    },
    Ficha {
        id: "nivelacion",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Reserva de nivelación: aplaza impuesto contra pérdidas futuras",
        resumen: "Las empresas de reducida dimensión pueden rebajar la base hasta un 10 % (como \
                  mucho 1 millón). Si en los cinco años siguientes hay pérdidas, esa rebaja las \
                  compensa; si no, se suma a la base del quinto año. Es impuesto aplazado, sin \
                  intereses.",
        requisitos: &[
            "Empresa de reducida dimensión: cifra de negocios del año anterior menor de 10 \
             millones, y no ser sociedad patrimonial.",
            "Dotar una reserva indisponible por el importe de la rebaja con el beneficio del año \
             (o, si no alcanza, con los primeros que haya). No puede ser la misma que la de \
             capitalización.",
            "Pendiente de aclarar sólo para las empresas de reducida dimensión de 1 a 10 millones: \
             la ley exige tributar al tipo del primer párrafo del art. 29.1, y desde 2025 su tipo \
             (23 % en 2026) está en otro párrafo; el de las microempresas (19 % y 21 %) sí está \
             en el primero.",
            "No vale mientras tributes al 15 % de nueva creación (DGT V3495-19).",
        ],
        pasos: &[
            "Si no eres microempresa, confirma con tu asesor que puedes aplicarla con tu tipo.",
            "En el Impuesto sobre Sociedades, aplica la rebaja (10 % de la base imponible después \
             de la reserva de capitalización y de compensar bases negativas, la de la casilla \
             [00552]) en la casilla [01034].",
            "En la junta que aprueba las cuentas, dota la «Reserva de nivelación» con ese importe.",
            "Apunta el año: la rebaja se suma con la primera base negativa de los cinco años \
             siguientes (casilla [01033]) o, si no la hay, a la base del quinto.",
        ],
        riesgos: &[
            "Si la reserva no se dota o se usa antes de tiempo, hay que pagar el impuesto \
             rebajado más un 5 % y los intereses de demora.",
            "No es un ahorro definitivo: si los cinco años son de beneficio, se paga en el quinto.",
        ],
        norma: "Art. 105 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a105",
        pantalla: Some("modelos"),
        aplazado: true,
        evaluar: nivelacion,
    },
    Ficha {
        id: "participadas",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Dividendos de tus participadas: el 95 % no tributa",
        resumen: "Los dividendos y las plusvalías de sociedades en las que tienes al menos el 5 % \
                  desde hace un año están exentos en un 95 % (el 100 % de los dividendos de una \
                  filial creada desde 2021 de la que tienes el 100 % desde el principio, cobrados \
                  en los tres años siguientes al de su creación, si tu cifra de negocios es menor \
                  de 40 millones, no eres sociedad patrimonial y antes de crearla no tenías el 5 % \
                  de otra sociedad ni formabas parte de un grupo). Hay que indicarlo en el \
                  Impuesto sobre Sociedades: si no, se paga de más.",
        requisitos: &[
            "Participación de al menos el 5 % (directa o indirecta).",
            "Tenerla al menos un año (se puede completar después del cobro).",
            "Si la participada es extranjera, que en su país tenga un impuesto parecido al de \
             Sociedades con un tipo nominal de al menos el 10 % (se presume si hay convenio).",
        ],
        pasos: &[
            "Reúne las actas o certificados de reparto de dividendos de cada participada y el \
             porcentaje que tienes.",
            "En el Impuesto sobre Sociedades, aplica la corrección en disminución por exención por \
             el 95 % de los dividendos: casilla [00370] si la participada es española, [02181] si \
             es extranjera.",
            "Si una participada te retuvo el 19 % sin tener que hacerlo, pídele que lo regularice o \
             réstalo como retención en tu 200.",
            "Si en años anteriores no se aplicó, pide la rectificación de esos 200 (cuatro años \
             desde el fin del plazo).",
        ],
        riesgos: &[
            "Sin la documentación del porcentaje y la antigüedad, la AEAT la rechaza.",
            "Si la participada vive a su vez de dividendos o plusvalías (más del 70 % de sus \
             ingresos), hay requisitos adicionales.",
        ],
        norma: "Art. 21 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a21",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: exencion_participadas,
    },
    Ficha {
        id: "tipo_reducido",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Aplica el tipo reducido de las empresas pequeñas",
        resumen: "Las sociedades con cifra de negocios del año anterior menor de 1 millón \
                  tributan menos los primeros 50.000 € de base y algo más el resto (19 % y 21 % \
                  en 2026; 17 % y 20 % en 2027); las de menos de 10 millones, al 23 % en 2026 y \
                  al 22 % en 2027. La Ley 7/2024 los baja cada año hasta 2029; el tipo general \
                  es el 25 %.",
        requisitos: &[
            "Cifra de negocios del periodo anterior menor de 1 millón (microempresa) o de 10 \
             millones (reducida dimensión), elevada al año si el periodo fue más corto. Si la \
             sociedad forma parte de un grupo (art. 42 del Código de Comercio, también con \
             sociedades de tu familia), cuenta la cifra del grupo.",
            "No ser sociedad patrimonial (más de la mitad del activo en valores o bienes no afectos \
             a la actividad): tributa al 25 %.",
        ],
        pasos: &[
            "En TaxPain, Modelos → «Datos de los modelos», elige el régimen que te corresponde.",
            "En el 200, la cifra de negocios menor de 1 millón se marca en la casilla [00088].",
            "Si te presenta el 200 la gestoría, comprueba que la casilla del tipo ([00558]) es \
             la del tipo reducido.",
        ],
        riesgos: &[
            "Si más de la mitad del activo de la sociedad (media de los balances trimestrales) son \
             valores o bienes que no se usan en la actividad —por ejemplo, pisos alquilados sin \
             una persona empleada a jornada completa, o el dinero de la empresa invertido en \
             fondos o acciones—, es sociedad patrimonial: tributa al 25 % y no puede aplicar los \
             tipos reducidos. Las participaciones de al menos el 5 % en sociedades con actividad, \
             gestionadas con medios propios, no cuentan.",
        ],
        norma: "Art. 29.1 y disposición transitoria 44.ª de la Ley 27/2014",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a29",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: tipo_reducido,
    },
    Ficha {
        id: "bases_negativas",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Compensa las pérdidas de años anteriores",
        resumen: "Las bases imponibles negativas de años anteriores (las pérdidas fiscales) se \
                  restan de la base de los años con beneficio, sin límite de tiempo: hasta 1 \
                  millón al año y, por encima, hasta el 70 % de la base.",
        requisitos: &[
            "Tener bases imponibles negativas declaradas en Impuestos sobre Sociedades \
             anteriores (la base negativa del 200, no la pérdida contable).",
            "Conservar los 200 y la contabilidad de los años en que se generaron: Hacienda puede \
             comprobarlas durante diez años.",
        ],
        pasos: &[
            "Suma las bases negativas de los 200 anteriores y resta las que ya compensaste.",
            "En TaxPain, Modelos → «Datos de los modelos»: indica las bases negativas pendientes \
             al empezar el ejercicio.",
            "En el 200, compénsalas en la casilla [00547] y detállalas por año de origen.",
        ],
        riesgos: &[
            "Si la sociedad cambió de socios y estaba inactiva, hay limitaciones (art. 26.4 de la \
             Ley 27/2014).",
        ],
        norma: "Art. 26 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a26",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: bases_negativas,
    },
    Ficha {
        id: "nueva_creacion",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Sociedad nueva: 15 % los dos primeros años con beneficio",
        resumen: "Las sociedades de nueva creación tributan al 15 % en el primer periodo con base \
                  positiva y en el siguiente, en lugar del tipo normal.",
        requisitos: &[
            "Que la actividad sea nueva: no vale si ya la hacían personas o empresas vinculadas, \
             ni si el año anterior la hacía como autónomo un socio con más del 50 %.",
            "No formar parte de un grupo de sociedades ni ser una sociedad patrimonial.",
        ],
        pasos: &[
            "Confirma la fecha de constitución y el primer año con base imponible positiva.",
            "En TaxPain, marca el régimen «Entidad de nueva creación» en «Datos de los modelos» \
             para esos dos años.",
            "En el 200, marca el carácter de entidad de nueva creación (casilla [00071]).",
        ],
        riesgos: &[
            "Si la actividad venía de un autónomo o de otra sociedad vinculada, la AEAT lo \
             rechaza.",
        ],
        norma: "Art. 29.1 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a29",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: nueva_creacion,
    },
    Ficha {
        id: "empresa_emergente",
        para: Para::Sociedad,
        area: Area::Beneficio,
        titulo: "Empresa emergente (startup): 15 % cuatro años",
        resumen: "Las empresas emergentes certificadas por ENISA tributan al 15 % el primer año \
                  con beneficio y los tres siguientes, pueden aplazar sin garantías ni intereses \
                  el impuesto de los dos primeros años con beneficio (12 y 6 meses) y no hacen \
                  pagos fraccionados en los siguientes.",
        requisitos: &[
            "Menos de cinco años desde la inscripción en el Registro Mercantil (siete en \
             biotecnología, energía, industria y otros sectores estratégicos).",
            "Modelo de negocio innovador y escalable, evaluado por ENISA.",
            "No repartir ni haber repartido dividendos.",
            "Sede social o establecimiento permanente en España y cifra de negocios de hasta 10 \
             millones.",
            "Al menos el 60 % de la plantilla con contrato laboral en España.",
            "No venir de una fusión, escisión o transformación de empresas que no eran \
             emergentes, ni cotizar en bolsa.",
        ],
        pasos: &[
            "Pide a ENISA la evaluación como empresa emergente (por su sede electrónica).",
            "Con la certificación, inscríbela en el Registro Mercantil.",
            "En el 200 de los cuatro años, marca el carácter de empresa emergente (casilla \
             [00083]) y aplica el 15 %. En TaxPain puedes poner el régimen de nueva creación para \
             que el borrador calcule el 15 %, pero el borrador marca la casilla de nueva creación \
             ([00071]): pide a quien presente el 200 que la cambie por la [00083].",
            "Al presentar el 200 de los dos primeros años con beneficio, pide el aplazamiento sin \
             garantías.",
        ],
        riesgos: &[
            "Si reparte dividendos, supera los 10 millones o pasa el plazo, deja de ser empresa \
             emergente y pierde las ventajas desde ese año.",
        ],
        norma: "Arts. 3, 7 y 8 de la Ley 28/2022 de empresas emergentes",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2022-21739",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: empresa_emergente,
    },
    Ficha {
        id: "audiovisual",
        para: Para::Ambos,
        area: Area::Inversion,
        titulo: "Financiar cine, series o espectáculos y desgravarlo",
        resumen: "Una empresa (o un autónomo) que aporta dinero a una producción española de \
                  cine, serie o espectáculo en vivo se aplica la deducción que genera la \
                  producción, hasta el 120 % de lo aportado: recuperas lo aportado en el \
                  impuesto y ganas hasta un 20 %. No compras derechos sobre la obra: sólo la \
                  deducción. Productoras y asesoras especializadas organizan estas operaciones.",
        requisitos: &[
            "Tener cuota del impuesto el año en que se aplica la deducción: el límite es el 25 % \
             de la cuota, y sube al 50 % si la deducción del financiador llega al 25 %.",
            "No estar vinculado con la productora (por ejemplo, tener el 25 % o más de ella).",
            "Contrato de financiación con la productora: partes, obra, presupuesto (gasto en \
             España, copias y publicidad) y cómo se financia.",
            "Aportar el dinero antes de que la producción obtenga los certificados de \
             nacionalidad y de carácter cultural (ICAA o, en espectáculos, INAEM).",
            "La deducción se aplica a partir del ejercicio en que la producción está terminada \
             (con la posproducción, no al acabar el rodaje); en las de animación, a partir del \
             ejercicio en que obtiene el certificado de nacionalidad. Nunca antes.",
            "Presentar a la AEAT, antes de que acabe el ejercicio en que tienes derecho a aplicar \
             la deducción, el contrato y los certificados de nacionalidad y de carácter cultural \
             del ICAA (o del órgano de la comunidad autónoma) con la entrega de la copia a la \
             Filmoteca (en espectáculos, el certificado del INAEM). Sin esa comunicación se pierde, aunque la cuota no te deje aplicarla \
             entera ese año.",
            "Los autónomos, sólo si tienen actividad económica (en la Renta, casillas [1722] y \
             [1723]).",
        ],
        pasos: &[
            "En septiembre u octubre, calcula la cuota del impuesto del año (TaxPain → Modelos → \
             200) y cuánto podrías aportar.",
            "Pide propuestas a productoras o asesoras especializadas en la financiación de \
             producciones (art. 39.7) y compáralas: en qué año termina la producción, \
             rentabilidad, solvencia y garantías si el proyecto se retrasa o cuesta menos.",
            "Revisa que no estás vinculado con la productora y que la producción tendrá los \
             certificados del ICAA o del INAEM.",
            "Firma el contrato de financiación antes de pagar.",
            "Paga la aportación por transferencia antes de que la producción obtenga los \
             certificados (y antes del cierre si quieres aplicarla este año).",
            "Presenta en la Sede la «comunicación del art. 39.7 de la Ley del Impuesto sobre \
             Sociedades», firmada por la productora y por ti, con el contrato, los certificados \
             del ICAA (o del INAEM) y el justificante de la copia entregada a la Filmoteca, antes \
             de que termine el ejercicio en que tienes derecho a aplicar la deducción.",
            "En el 200, marca el carácter de financiador (casilla [00074]) y aplica la deducción \
             en «Financiador: producciones cinematográficas españolas» ([02463]; en espectáculos, \
             [02456]), con el NIF de la productora.",
            "Guarda contrato, justificante de pago, comunicación a la AEAT y certificados diez \
             años desde que acaba el plazo del impuesto del año en que se genera la deducción: es \
             el tiempo que tiene Hacienda para comprobarla (art. 66 bis de la Ley General \
             Tributaria).",
        ],
        riesgos: &[
            "Si la producción no termina este año, no obtiene los certificados o cuesta menos de \
             lo presupuestado, la deducción baja o se retrasa: busca contratos con garantía.",
            "Lo que no quepa en el límite de la cuota se aplica en los 15 años siguientes.",
            "Cómo se contabiliza la aportación y si es gasto deducible no está aclarado por \
             Hacienda: que lo confirme tu asesor antes de firmar.",
            "La deducción comparte el límite con otras (por ejemplo, la de I+D).",
        ],
        norma: "Arts. 36 y 39.7 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a39",
        pantalla: Some("modelos"),
        aplazado: false,
        evaluar: audiovisual,
    },
    Ficha {
        id: "i_mas_d",
        para: Para::Ambos,
        area: Area::Inversion,
        titulo: "Deducción por I+D e innovación tecnológica",
        resumen: "Los gastos en investigación y desarrollo desgravan un 25 % (42 % por encima de la \
                  media de los dos años anteriores, más un 17 % del personal investigador en \
                  exclusiva) y los de innovación tecnológica un 12 %. El desarrollo de software \
                  nuevo puede entrar.",
        requisitos: &[
            "Proyectos con novedad objetiva (no mejoras rutinarias) y documentados.",
            "Llevar los gastos del proyecto separados (horas del personal, colaboradores, \
             materiales).",
            "Muy recomendable: un informe motivado del ministerio competente en ciencia e \
             innovación, que vincula a la AEAT.",
        ],
        pasos: &[
            "Identifica los proyectos del año y describe en qué son nuevos.",
            "Registra las horas y gastos de cada proyecto (partes de horas, facturas).",
            "Pide la certificación a una entidad acreditada por ENAC y el informe motivado.",
            "Los equipos e intangibles que uses sólo para I+D (no los edificios) y los gastos de \
             I+D que actives se pueden amortizar libremente (art. 12.3.b y c de la Ley 27/2014).",
            "Aplica la deducción en el Impuesto sobre Sociedades (capítulo IV de deducciones), con \
             el límite del 25 % de la cuota (50 % si la de I+D+i pasa del 10 %). Lo que no quepa, \
             en los 18 años siguientes, o pide su abono con un descuento del 20 %.",
        ],
        riesgos: &[
            "Sin informe motivado, la AEAT puede discutir que el proyecto sea I+D o innovación.",
            "La bonificación de la Seguridad Social del personal investigador no se puede sumar a \
             esta deducción por la misma persona, salvo que la empresa tenga el sello de PYME \
             innovadora.",
        ],
        norma: "Arts. 35 y 39 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a35",
        pantalla: None,
        aplazado: false,
        evaluar: i_mas_d,
    },
    Ficha {
        id: "patent_box",
        para: Para::Sociedad,
        area: Area::Inversion,
        titulo: "Patent box: hasta el 60 % de lo que ganas con software o patentes propias",
        resumen: "Las rentas por ceder el uso (licencias) de patentes, modelos de utilidad, \
                  diseños protegidos o software avanzado registrado que haya desarrollado la \
                  propia empresa reducen la base un 60 %, en proporción a los gastos de \
                  desarrollo propios.",
        requisitos: &[
            "Intangible creado por la empresa (la reducción se ajusta por la proporción de gastos \
             propios y subcontratados con no vinculados).",
            "Software avanzado registrado que derive de un proyecto de I+D (las marcas y el \
             software normal no entran).",
            "Contabilidad separada de ingresos y gastos del intangible, y contrato que separe las \
             prestaciones accesorias.",
        ],
        pasos: &[
            "Identifica qué software o patente propia licencias y qué ingresos da.",
            "Regístralo (Registro de la Propiedad Intelectual u oficina de patentes).",
            "Calcula la renta neta con los gastos del desarrollo y aplica la reducción en el 200 \
             (casilla [00372]).",
            "Valora un acuerdo previo de valoración con la AEAT si las cifras son altas.",
        ],
        riesgos: &["La AEAT revisa el cálculo de la renta y la proporción de gastos propios."],
        norma: "Art. 23 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a23",
        pantalla: None,
        aplazado: false,
        evaluar: patent_box,
    },
    Ficha {
        id: "escaso_valor",
        para: Para::Ambos,
        area: Area::Gastos,
        titulo: "Compras de hasta 300 €: gasto entero este año",
        resumen: "Los equipos, muebles y herramientas nuevos de hasta 300 € la unidad se pueden \
                  amortizar enteros el año de compra (hasta 25.000 € al año), en lugar de en \
                  varios años.",
        requisitos: &[
            "Elementos nuevos del inmovilizado material de 300 € o menos la unidad.",
            "Puestos a disposición dentro del ejercicio.",
        ],
        pasos: &[
            "Si ibas a comprar material pronto, hazlo antes del cierre del ejercicio.",
            "Regístralo en el inmovilizado y aplica la libertad de amortización (en el 200, \
             casillas [01003]/[01004] si la contable es distinta).",
            "Guarda las facturas con el detalle por unidad.",
        ],
        riesgos: &["Comprar sólo por desgravar no compensa: adelanta el gasto, no lo crea."],
        norma: "Art. 12.3.e) de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a12",
        pantalla: Some("inmovilizado"),
        aplazado: true,
        evaluar: escaso_valor,
    },
    Ficha {
        id: "amortizacion_acelerada",
        para: Para::Ambos,
        area: Area::Gastos,
        titulo: "Amortización acelerada de inversiones nuevas",
        resumen: "Las empresas de reducida dimensión (y los autónomos en estimación directa) \
                  pueden amortizar el inmovilizado material, las inversiones inmobiliarias y el \
                  intangible nuevos al doble del coeficiente máximo de las tablas. Si además \
                  aumentan la plantilla, pueden amortizar libremente hasta 120.000 € por cada \
                  persona de aumento.",
        requisitos: &[
            "Elementos nuevos, comprados siendo empresa de reducida dimensión.",
            "Para la libertad de amortización: aumento de la plantilla media en los 24 meses \
             siguientes, mantenido otros 24.",
        ],
        pasos: &[
            "Registra la compra en Contabilidad → Inmovilizado.",
            "Amortiza al doble del coeficiente de tablas y apunta en el 200 la diferencia entre \
             la amortización contable y la fiscal si son distintas (casillas [00313]/[00314]; la \
             libertad por empleo, [00311]/[00312]).",
        ],
        riesgos: &[
            "Al vender el bien, la plusvalía fiscal es mayor.",
            "Si no se mantiene la plantilla, hay que devolver el impuesto con intereses.",
        ],
        norma: "Arts. 102 y 103 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a103",
        pantalla: Some("inmovilizado"),
        aplazado: true,
        evaluar: amortizacion_acelerada,
    },
    Ficha {
        id: "vehiculos_renovables",
        para: Para::Ambos,
        area: Area::Gastos,
        titulo: "Coche eléctrico, punto de recarga o placas solares: amortización libre",
        resumen: "Los vehículos eléctricos o híbridos enchufables nuevos afectos a la actividad, \
                  los puntos de recarga y las instalaciones de autoconsumo renovable que entren \
                  en funcionamiento en 2026 se pueden amortizar enteros ese mismo año.",
        requisitos: &[
            "Vehículos nuevos FCV, FCHV, BEV, REEV o PHEV, afectos a la actividad.",
            "Puntos de recarga nuevos, con proyecto o memoria y el certificado de la comunidad \
             autónoma.",
            "Autoconsumo eléctrico renovable o instalaciones térmicas renovables que sustituyan \
             combustibles fósiles: hasta 500.000 € y manteniendo la plantilla 24 meses, con la \
             documentación de la instalación (certificado eléctrico e inscripción en el \
             registro).",
            "Coches y puntos de recarga: que entren en funcionamiento en un ejercicio iniciado en \
             2024, 2025 o 2026 (en la práctica, antes del cierre de tu ejercicio actual). Placas y \
             demás autoconsumo: que entren en funcionamiento dentro de 2026 (como tarde el 31 de \
             diciembre). No consta prórroga para 2027.",
        ],
        pasos: &[
            "Pide presupuesto y asegúrate de que la entrega o la instalación termina antes del \
             cierre.",
            "Regístralo en el inmovilizado y aplica la libertad de amortización: en el 200, \
             casillas [00005]/[00006] (vehículos y puntos de recarga) o [00309]/[00310] \
             (autoconsumo).",
            "Guarda la factura, el certificado de la instalación y, en el coche, la prueba del \
             uso para la actividad.",
        ],
        riesgos: &[
            "La afectación del coche es lo que más revisa la AEAT: tiene que usarse para la \
             actividad.",
            "Adelanta el gasto: en los años siguientes ya no hay amortización de ese bien.",
        ],
        norma: "Disposiciones adicionales 17.ª y 18.ª de la Ley 27/2014 (prorrogadas para 2026 \
                por el art. 37 del Real Decreto-ley 7/2026)",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328",
        pantalla: Some("inmovilizado"),
        aplazado: true,
        evaluar: vehiculos_renovables,
    },
    Ficha {
        id: "deterioro_creditos",
        para: Para::Sociedad,
        area: Area::Gastos,
        titulo: "Clientes que no pagan: gasto deducible",
        resumen: "Las facturas que al cierre llevan más de seis meses vencidas sin cobrar se \
                  pueden contabilizar como pérdida por deterioro y restarlas del beneficio. Las \
                  empresas de reducida dimensión pueden además restar el 1 % del saldo de \
                  clientes como deterioro global.",
        requisitos: &[
            "Seis meses desde el vencimiento al cierre del ejercicio (o deudor en concurso o \
             reclamado judicialmente).",
            "No vale con administraciones públicas (salvo litigio) ni con empresas vinculadas.",
            "Tiene que estar contabilizado: si no, no es deducible.",
        ],
        pasos: &[
            "Revisa Contabilidad → Cobros y pagos y reclama las facturas vencidas.",
            "Al cierre, contabiliza el deterioro (cargo en la 694 y abono en la 490) de las que \
             lleven seis meses vencidas, y el 1 % global si eres empresa de reducida dimensión.",
            "En el 200, si la contable y la fiscal no coinciden, casillas [00321]/[00322] (y \
             [00323]/[00324] el 1 % global).",
            "Si te pagan después, el deterioro se revierte como ingreso.",
        ],
        riesgos: &["Hay que poder demostrar las gestiones de cobro."],
        norma: "Arts. 13.1 y 104 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a13",
        pantalla: Some("cartera"),
        aplazado: false,
        evaluar: deterioro_creditos,
    },
    Ficha {
        id: "iva_impagadas",
        para: Para::Ambos,
        area: Area::Iva,
        titulo: "Recupera el IVA de las facturas que no te pagan",
        resumen: "Si una factura emitida lleva 6 meses sin cobrar (un año en las empresas \
                  grandes) y has reclamado el cobro de forma fehaciente, puedes emitir una \
                  rectificativa y recuperar el IVA que ya ingresaste.",
        requisitos: &[
            "Plazo desde el devengo: 6 meses o un año si tu volumen de operaciones del año \
             anterior fue de 6.010.121,04 € o menos; un año si fue mayor.",
            "Haber reclamado el cobro por vía judicial, con requerimiento notarial o por otro \
             medio fehaciente (por ejemplo, burofax con certificación de contenido).",
            "Factura anotada en tu libro de emitidas.",
            "Si el cliente no es empresario, que la base sea de más de 50 €.",
            "No vale con créditos garantizados o asegurados, con empresas vinculadas ni con \
             clientes no establecidos en España (si un cliente de otro país de la UE está en un \
             procedimiento de insolvencia, el IVA se recupera por la vía del concurso, no por \
             esta).",
        ],
        pasos: &[
            "Reclama la deuda por burofax con certificación de contenido, requerimiento notarial o \
             demanda (o un monitorio).",
            "Emite la factura rectificativa por créditos incobrables (tipo R3), que rebaja la base \
             y el IVA de lo que no te han pagado, y envíasela al cliente, en los 6 meses \
             siguientes al final del plazo de 6 meses o un año.",
            "Comunícalo a la AEAT en el mes siguiente a la rectificativa, con el formulario de la \
             Sede para la modificación de la base imponible, adjuntando la rectificativa y la \
             prueba de la reclamación.",
            "Resta el IVA en el 303 del periodo de la rectificativa.",
        ],
        riesgos: &[
            "Fuera de plazo no se puede recuperar: mira la fecha límite.",
            "Si luego cobras de una empresa, no hay que volver a ingresar el IVA; si el cliente es \
             un particular, sí (va incluido en lo que cobres).",
            "Si desistes de la reclamación judicial o llegas a un acuerdo de cobro, hay que emitir \
             otra rectificativa al alza en un mes.",
            "Si el cliente está en concurso, el plazo y el procedimiento son otros.",
        ],
        norma: "Art. 80.Cuatro de la Ley 37/1992 del IVA y art. 24 del Reglamento del IVA",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-1992-28740#a80",
        pantalla: Some("cartera"),
        aplazado: false,
        evaluar: iva_impagadas,
    },
    Ficha {
        id: "plan_pensiones_empleo",
        para: Para::Sociedad,
        area: Area::Personas,
        titulo: "Plan de pensiones de empleo para la plantilla",
        resumen: "La empresa aporta a un plan de pensiones de empleo (o a un plan de previsión \
                  social empresarial) a favor de sus trabajadores. Es gasto deducible para la \
                  sociedad, desgrava un 10 % de lo aportado (en proporción a 27.000 € si el \
                  trabajador cobra más) y cada trabajador lo resta de su Renta.",
        requisitos: &[
            "Plan de empleo promovido por la empresa y abierto a la plantilla: no puede ser sólo \
             para los socios o administradores.",
            "Pueden entrar los trabajadores con relación laboral y, en un plan de pensiones de \
             empleo, también los administradores que cotizan en el Régimen General como asimilados \
             (no controlan la sociedad, hacen funciones de dirección y cobran por ello: art. 25.2 \
             del Reglamento de planes y fondos de pensiones). El socio que controla la sociedad \
             (cotiza como autónomo) no puede entrar aunque cobre nómina. En un plan de previsión \
             social empresarial sólo entra quien tiene relación laboral (DGT V0618-26).",
            "Límite en la Renta de cada persona: 1.500 € de sus aportaciones más 8.500 € de las \
             de la empresa (10.000 € en total y no más del 30 % de sus rendimientos).",
        ],
        pasos: &[
            "Pide a tu banco o gestora un plan de empleo (o un plan simplificado sectorial).",
            "Fija en el reglamento del plan cuánto aporta la empresa por persona.",
            "Haz las aportaciones antes del 31 de diciembre y contabilízalas (gasto de personal).",
            "Incluye las aportaciones en las nóminas (retribución en especie) y en el 190.",
        ],
        riesgos: &[
            "El dinero no se puede rescatar hasta la jubilación (salvo los supuestos legales).",
        ],
        norma: "Arts. 51 y 52 de la Ley 35/2006 del IRPF y art. 38 ter de la Ley 27/2014",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a51",
        pantalla: None,
        aplazado: false,
        evaluar: plan_pensiones_empleo,
    },
    Ficha {
        id: "retribucion_flexible",
        para: Para::Sociedad,
        area: Area::Personas,
        titulo: "Retribución flexible para el equipo",
        resumen: "Pagar parte del sueldo en seguro médico (hasta 500 € al año por persona), \
                  comida (11 € al día laborable), transporte público (1.500 € al año) o guardería: \
                  no tributa en la Renta del trabajador y sigue siendo gasto para la empresa.",
        requisitos: &[
            "Acordarlo con cada trabajador (cambia sueldo dinerario por estos servicios).",
            "Pagar al proveedor directamente o con tarjetas o vales nominativos.",
            "Lo que se paga en especie no puede pasar del 30 % del sueldo de cada trabajador ni \
             dejar lo que cobra en dinero por debajo del salario mínimo (art. 26.1 del Estatuto de \
             los Trabajadores).",
        ],
        pasos: &[
            "Elige los conceptos (seguro médico, comida, transporte, guardería, formación).",
            "Firma con cada trabajador el acuerdo de retribución flexible.",
            "Contrata el seguro o las tarjetas y refleja los importes en nómina.",
        ],
        riesgos: &["Lo que pase de los límites tributa como sueldo en especie."],
        norma: "Art. 42 de la Ley 35/2006 del IRPF y arts. 44 a 46 bis de su Reglamento",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a42",
        pantalla: None,
        aplazado: false,
        evaluar: retribucion_flexible,
    },
    Ficha {
        id: "retribucion_administrador",
        para: Para::Sociedad,
        area: Area::Personas,
        titulo: "Socio-administrador: retribución o dividendos",
        resumen: "Lo que la sociedad paga al administrador por su cargo es gasto deducible en el \
                  Impuesto sobre Sociedades y tributa en su Renta como rendimiento del trabajo; el \
                  dividendo paga primero Sociedades y después la Renta del ahorro. Según el tipo \
                  marginal del socio, una vía sale más barata que la otra.",
        requisitos: &[
            "Que los estatutos digan que el cargo de administrador es retribuido y cómo (art. 217 \
             de la Ley de Sociedades de Capital).",
            "Que la junta fije el importe máximo anual.",
            "Retención en la nómina del 35 % (19 % si la cifra de negocios de la sociedad es menor \
             de 100.000 €).",
        ],
        pasos: &[
            "Revisa los estatutos: si no dicen que el cargo es retribuido, modifícalos en junta \
             ante notario e inscríbelo en el Registro Mercantil.",
            "La junta fija el importe máximo de la retribución del año.",
            "Paga la retribución con su retención, declárala en el 111 cada trimestre y en el 190 \
             en enero.",
            "Compara cada año con tu asesor cuánto cobrar como retribución y cuánto como \
             dividendo, según tu tipo marginal en la Renta.",
        ],
        riesgos: &[
            "Sin estatutos que lo prevean, la AEAT puede no admitir el gasto.",
            "Si además prestas servicios profesionales a la sociedad, tu retribución puede ser \
             rendimiento de actividad económica: consúltalo.",
        ],
        norma: "Art. 217 de la Ley de Sociedades de Capital y art. 15.e) de la Ley 27/2014",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a15",
        pantalla: None,
        aplazado: false,
        evaluar: retribucion_administrador,
    },
    Ficha {
        id: "discapacidad",
        para: Para::Sociedad,
        area: Area::Personas,
        titulo: "Contratar personas con discapacidad",
        resumen: "Cada persona con discapacidad que aumenta la plantilla media desgrava 9.000 € \
                  (33 % a 65 %) o 12.000 € (65 % o más) en el Impuesto sobre Sociedades.",
        requisitos: &[
            "Aumento de la plantilla media de trabajadores con discapacidad frente al año \
             anterior.",
        ],
        pasos: &[
            "Contacta con el servicio público de empleo o entidades de inserción.",
            "Calcula la plantilla media con discapacidad de este año y del anterior.",
            "Aplica la deducción en el 200, con el límite del 25 % de la cuota.",
        ],
        riesgos: &["Se calcula con la plantilla media, no con las altas."],
        norma: "Art. 38 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a38",
        pantalla: None,
        aplazado: false,
        evaluar: discapacidad,
    },
    Ficha {
        id: "donativos_sociedad",
        para: Para::Sociedad,
        area: Area::Donativos,
        titulo: "Donativos y mecenazgo",
        resumen: "Las donaciones a entidades acogidas a la Ley de mecenazgo (ONG, fundaciones, \
                  universidades…) desgravan el 40 % de lo donado (50 % si donaste a esa misma \
                  entidad en los dos años anteriores y no bajas el importe: lo de este año igual o \
                  más que lo del año pasado, y lo del año pasado igual o más que lo del anterior; \
                  5 puntos más en las actividades prioritarias de mecenazgo), con el límite del 15 \
                  % de la base.",
        requisitos: &[
            "Entidad acogida a la Ley 49/2002 (pide su certificado).",
            "Donación irrevocable y sin contraprestación (si la hay, es patrocinio), pagada antes \
             del cierre.",
        ],
        pasos: &[
            "Elige la entidad y confirma que está acogida a la Ley 49/2002.",
            "Haz la donación por transferencia antes del cierre del ejercicio.",
            "Pide el certificado de la donación (la entidad lo declara en su modelo 182).",
            "El donativo no es gasto: súmalo como corrección en aumento y aplica la deducción en \
             el 200 (casilla [00565]).",
        ],
        riesgos: &["Lo que no quepa en el límite se aplica en los diez años siguientes."],
        norma: "Art. 20 de la Ley 49/2002 de mecenazgo",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2002-25039#a20",
        pantalla: None,
        aplazado: false,
        evaluar: donativos_sociedad,
    },
    Ficha {
        id: "plan_pensiones_autonomo",
        para: Para::Autonomo,
        area: Area::Personas,
        titulo: "Planes de pensiones: rebaja la Renta",
        resumen: "Lo aportado a planes de pensiones se resta de la base de la Renta: 1.500 € al \
                  año en planes individuales y hasta 4.250 € más en planes de empleo simplificados \
                  para autónomos (5.750 € en total).",
        requisitos: &[
            "Aportar antes del 31 de diciembre.",
            "Límite: 30 % de los rendimientos netos del trabajo y de la actividad.",
        ],
        pasos: &[
            "Pide a tu banco o gestora un plan individual y un plan de empleo simplificado para \
             autónomos.",
            "Aporta antes del 31 de diciembre (mejor a principios de mes para que dé tiempo).",
            "Guarda el certificado de aportaciones: los datos fiscales de la Renta las traen \
             (casillas [0465] y [0499]).",
        ],
        riesgos: &[
            "Al rescatarlo tributa como rendimiento del trabajo: conviene si tu tipo hoy es mayor \
             que el que tendrás al jubilarte.",
            "No se puede rescatar hasta la jubilación (salvo con 10 años de antigüedad y otros \
             supuestos legales).",
        ],
        norma: "Arts. 51 y 52 de la Ley 35/2006 del IRPF",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a52",
        pantalla: None,
        aplazado: false,
        evaluar: plan_pensiones_autonomo,
    },
    Ficha {
        id: "inicio_actividad",
        para: Para::Autonomo,
        area: Area::Beneficio,
        titulo: "Autónomo nuevo: 20 % menos de rendimiento",
        resumen: "Los autónomos en estimación directa que empiezan una actividad reducen un 20 % el \
                  rendimiento neto (hasta 100.000 €) el primer año con beneficio y el siguiente.",
        requisitos: &[
            "Estimación directa.",
            "No haber tenido actividad económica en el año anterior al alta.",
        ],
        pasos: &[
            "Confirma la fecha de alta y que el año anterior no tuviste otra actividad.",
            "En la Renta, aplica la reducción por inicio de actividad (casilla [0234]).",
        ],
        riesgos: &[
            "No vale el año en que más de la mitad de los ingresos vengan de quien era tu \
             empleador el año anterior.",
        ],
        norma: "Art. 32.3 de la Ley 35/2006 del IRPF",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a32",
        pantalla: None,
        aplazado: false,
        evaluar: inicio_actividad,
    },
    Ficha {
        id: "gastos_autonomo",
        para: Para::Autonomo,
        area: Area::Gastos,
        titulo: "Gastos de autónomo que se suelen olvidar",
        resumen: "En estimación directa, los suministros de tu vivienda habitual si trabajas en \
                  ella, el seguro médico, las comidas en días de trabajo y la cuota de autónomos \
                  son gasto deducible en la Renta.",
        requisitos: &[
            "Estimación directa.",
            "Suministros: parte de tu vivienda habitual afecta a la actividad, declarada en el \
             censo (modelo 036).",
            "Comidas: en restaurantes, en días de trabajo, pagadas con tarjeta u otro medio \
             electrónico.",
        ],
        pasos: &[
            "Declara en el censo (modelo 036) la parte de la vivienda que usas para trabajar.",
            "Registra como gasto el 30 % de esa proporción de luz, agua, gas, teléfono e \
             internet.",
            "Añade el seguro médico (500 € por persona: tú, tu cónyuge e hijos menores de 25 que \
             vivan contigo; 1.500 € con discapacidad).",
            "Registra las comidas en días de trabajo: hasta 26,67 € al día en España y 48,08 € en \
             el extranjero (53,34 € y 91,35 € si duermes fuera).",
        ],
        riesgos: &["Sin factura a tu nombre y pago con tarjeta, la AEAT no las admite."],
        norma: "Art. 30.2 de la Ley 35/2006 del IRPF",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a30",
        pantalla: Some("recibidas"),
        aplazado: false,
        evaluar: gastos_autonomo,
    },
    Ficha {
        id: "empresas_nuevas",
        para: Para::Autonomo,
        area: Area::Inversion,
        titulo: "Invertir en empresas nuevas desgrava el 50 %",
        resumen: "Suscribir participaciones o acciones de una empresa de nueva o reciente creación \
                  desgrava el 50 % en la parte estatal de la Renta (hasta 100.000 € de inversión \
                  al año).",
        requisitos: &[
            "Sociedad no cotizada con actividad económica, de menos de cinco años (siete si es \
             empresa emergente) y con fondos propios de 400.000 € o menos.",
            "Suscribir en la constitución o en una ampliación de capital.",
            "No tener más del 40 % con tu familia y mantener la inversión entre 3 y 12 años.",
        ],
        pasos: &[
            "Pide a la empresa el certificado de que cumple los requisitos.",
            "Suscribe en la constitución o en una ampliación de capital ante notario.",
            "Aplica la deducción en la Renta del año de la inversión (casilla [0549], con el NIF \
             de la sociedad).",
        ],
        riesgos: &["Si vendes antes de tres años, se devuelve la deducción."],
        norma: "Art. 68.1 de la Ley 35/2006 del IRPF",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a68",
        pantalla: None,
        aplazado: false,
        evaluar: empresas_nuevas,
    },
    Ficha {
        id: "reinversion_autonomo",
        para: Para::Autonomo,
        area: Area::Inversion,
        titulo: "Reinvierte en la actividad y desgrava el 5 %",
        resumen: "Los autónomos en estimación directa se desgravan el 5 % de los rendimientos de la \
                  actividad que reinvierten en elementos nuevos del inmovilizado (el 2,5 % si \
                  aplican la reducción por inicio de actividad).",
        requisitos: &[
            "Estimación directa y cifra de negocios del año anterior menor de 10 millones.",
            "Elementos nuevos del inmovilizado material o inversiones inmobiliarias, afectos a la \
             actividad, comprados este año o el siguiente.",
            "Mantenerlos cinco años (o su vida útil si es menor).",
            "No es compatible con la libertad de amortización de esos mismos elementos.",
        ],
        pasos: &[
            "Compra el elemento con factura a tu nombre.",
            "Regístralo en Contabilidad → Inmovilizado y amortízalo con normalidad.",
            "En la Renta, aplica la deducción por reinversión de beneficios (incentivos a la \
             inversión empresarial) por el 5 % de lo invertido, hasta el rendimiento neto de la \
             actividad.",
        ],
        riesgos: &["Si lo vendes antes de cinco años, se devuelve la deducción con intereses."],
        norma: "Art. 68.2.b) de la Ley 35/2006 del IRPF",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a68",
        pantalla: Some("inmovilizado"),
        aplazado: false,
        evaluar: reinversion_autonomo,
    },
    Ficha {
        id: "donativos_autonomo",
        para: Para::Autonomo,
        area: Area::Donativos,
        titulo: "Donativos en la Renta",
        resumen: "Los primeros 250 € donados a entidades de la Ley de mecenazgo desgravan el 80 %; \
                  el resto, el 40 % (45 % si donaste a esa misma entidad en los dos años \
                  anteriores y no bajas el importe: lo de este año igual o más que lo del año \
                  pasado, y lo del año pasado igual o más que lo del anterior).",
        requisitos: &[
            "Entidad acogida a la Ley 49/2002.",
            "Límite: 10 % de la base liquidable.",
        ],
        pasos: &[
            "Dona antes del 31 de diciembre con tu NIF.",
            "Comprueba que la donación aparece en tus datos fiscales.",
        ],
        riesgos: &[],
        norma: "Art. 19 de la Ley 49/2002 de mecenazgo",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2002-25039#a19",
        pantalla: None,
        aplazado: false,
        evaluar: donativos_autonomo,
    },
    Ficha {
        id: "pasar_a_sociedad",
        para: Para::Autonomo,
        area: Area::Beneficio,
        titulo: "Rendimiento alto: valora pasar la actividad a una sociedad",
        resumen: "Con rendimientos altos, la Renta llega al 45-47 %. En una sociedad, el beneficio \
                  que no se reparte paga el Impuesto sobre Sociedades (del 17 % al 23 % en las \
                  pequeñas en 2026 y 2027); lo que se saca tributa después en la Renta del socio.",
        requisitos: &[
            "Que no necesites sacar todo el beneficio cada año: lo que sale como sueldo o \
             dividendo vuelve a tributar en tu Renta.",
            "Tener en cuenta los costes de la sociedad (constitución, contabilidad, cuentas \
             anuales, gestoría).",
        ],
        pasos: &[
            "Haz con tu asesor la comparación con tus cifras: impuesto como autónomo frente a \
             Sociedades más lo que vayas a sacar.",
            "Constituye la sociedad (notario y Registro Mercantil) y date de alta en el censo \
             (modelo 036).",
            "Traspasa a la sociedad los clientes, contratos y bienes de la actividad, y date de \
             baja como autónomo en la actividad. Ojo: aportar o vender los bienes a la sociedad \
             hace tributar su plusvalía en tu Renta. Puede aplazarse hasta que vendas las \
             participaciones con el régimen especial de los arts. 87 y 89 de la Ley 27/2014: los \
             bienes tienen que estar afectos a una actividad con contabilidad conforme al Código \
             de Comercio (si aportas bienes sueltos, quédate con al menos el 5 % de la sociedad), \
             la operación tiene que tener motivos económicos además del ahorro de impuestos, y la \
             sociedad tiene que comunicarla a Hacienda en los tres meses siguientes a inscribir la \
             escritura (si no, multa de 10.000 €). Hazlo con tu asesor.",
            "Fija tu retribución como socio que trabaja en la sociedad (operación vinculada).",
        ],
        riesgos: &[
            "Si la actividad la hacías tú (con más del 50 % de la sociedad), la sociedad no tiene \
             el 15 % de nueva creación.",
            "En actividades profesionales, lo que cobra el socio que presta los servicios tiene \
             reglas propias de valoración (operaciones vinculadas, art. 18.6 de la Ley 27/2014).",
        ],
        norma: "Arts. 18 y 29 de la Ley 27/2014 del Impuesto sobre Sociedades",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2014-12328#a18",
        pantalla: None,
        aplazado: false,
        evaluar: pasar_a_sociedad,
    },
    Ficha {
        id: "prorrata_especial",
        para: Para::Ambos,
        area: Area::Iva,
        titulo: "IVA con prorrata: compara la especial",
        resumen: "Quien hace a la vez operaciones con derecho a deducir y exentas aplica la \
                  prorrata general salvo que elija la especial: con ella deduce todo el IVA de lo \
                  que usa sólo en las operaciones con derecho a deducir, nada de lo que usa sólo \
                  en las exentas y la prorrata en lo común.",
        requisitos: &[
            "Actividad con operaciones con y sin derecho a deducir el IVA.",
            "La especial es obligatoria si con la general deducirías al menos un 10 % más que con \
             la especial.",
        ],
        pasos: &[
            "Separa el IVA soportado de cada compra: sólo para operaciones con derecho, sólo para \
             exentas o común (en TaxPain, en cada factura recibida).",
            "Calcula lo que deducirías con cada una con los datos del año.",
            "Si te conviene la especial, elígela en la última declaración del año (el 303 del \
             cuarto trimestre o de diciembre).",
        ],
        riesgos: &["Una vez elegida, se mantiene al menos tres años."],
        norma: "Arts. 103 y 106 de la Ley 37/1992 del IVA",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-1992-28740#a103",
        pantalla: Some("recibidas"),
        aplazado: false,
        evaluar: prorrata_especial,
    },
    Ficha {
        id: "reserva_baleares",
        para: Para::Ambos,
        area: Area::Beneficio,
        titulo: "Reserva para inversiones en las Illes Balears",
        resumen: "Las empresas y autónomos con establecimientos en las Illes Balears pueden rebajar \
                  la base hasta el 90 % del beneficio que dejan en una reserva, si lo invierten en \
                  los tres años siguientes en activos nuevos en las islas, en mejoras ambientales o \
                  en I+D (periodos de 2023 a 2028).",
        requisitos: &[
            "Beneficio de actividades de establecimientos situados en las Illes Balears, que no se \
             reparte (se dota la reserva).",
            "Invertirlo en los tres años siguientes en activos nuevos afectos a la actividad en \
             las islas, en mejora del medio ambiente o en I+D.",
            "En la Renta, estimación directa y contabilidad conforme al Código de Comercio; la \
             deducción es a tu tipo medio, con un límite del 80 % de la cuota de esos \
             rendimientos.",
            "Lo que se dote no cuenta para la reserva de capitalización.",
        ],
        pasos: &[
            "Calcula el beneficio del año que viene de los establecimientos en las islas.",
            "Dota la reserva al aprobar las cuentas (sociedades) o regístrala (autónomos).",
            "Aplica la rebaja en el 200 o la deducción en la Renta y guarda el plan de inversión.",
            "Invierte en los tres años siguientes y conserva las facturas y la prueba del uso en \
             las islas.",
        ],
        riesgos: &["Si no se invierte en plazo, hay que devolver el impuesto con intereses."],
        norma: "Disposición adicional 70.ª de la Ley 31/2022 y Real Decreto 710/2024",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2022-22128",
        pantalla: None,
        aplazado: false,
        evaluar: reserva_baleares,
    },
    Ficha {
        id: "cripto_perdidas",
        para: Para::Ambos,
        area: Area::Cripto,
        titulo: "Criptomonedas: compensa ganancias con pérdidas antes del 31 de diciembre",
        resumen: "Las ganancias de vender o cambiar criptomonedas tributan; las pérdidas las \
                  compensan. Si tienes ganancias este año y posiciones en pérdidas, venderlas \
                  antes del 31 de diciembre rebaja el impuesto.",
        requisitos: &[
            "Las pérdidas tienen que realizarse (vender o cambiar) dentro del año.",
            "Cada venta o permuta se calcula con el método FIFO (las primeras compradas, las \
             primeras vendidas).",
        ],
        pasos: &[
            "Importa todas las operaciones de tus plataformas en Criptomonedas.",
            "Pon los precios del día para ver qué posiciones están en pérdidas.",
            "Antes del 31 de diciembre, vende o cambia todas las unidades de cada moneda que \
             pierde en conjunto: con FIFO se venden primero las más antiguas y, si vendes sólo \
             una parte, puede salir ganancia en vez de pérdida.",
            "En la Renta, declara cada transmisión (casillas de monedas virtuales); si tienes más \
             de 50.000 € en plataformas del extranjero, presenta el 721. En una sociedad, la \
             pérdida queda en la contabilidad.",
        ],
        riesgos: &[
            "En la Renta: no está aclarado si, al volver a comprar la misma moneda antes de un \
             año, la pérdida deja de contar (art. 33.5.e de la Ley del IRPF); la regla de los dos \
             meses de las acciones no se aplica porque no son valores (DGT V0525-25). Consúltalo \
             antes de recomprar.",
            "En la Renta, las pérdidas que no compenses este año se pueden usar en los cuatro \
             siguientes; en una sociedad, la base negativa se compensa sin plazo.",
        ],
        norma: "Arts. 33, 46 y 49 de la Ley 35/2006 del IRPF (y Ley 27/2014 en sociedades)",
        enlace: "https://www.boe.es/buscar/act.php?id=BOE-A-2006-20764#a49",
        pantalla: Some("cripto"),
        aplazado: false,
        evaluar: cripto_perdidas,
    },
];
