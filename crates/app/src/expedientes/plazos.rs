//! Cómputo de los plazos de un expediente, del recargo del periodo ejecutivo y de las
//! reducciones de las sanciones. Cálculo puro (sin almacén): el calendario de festivos lo pone
//! quien llama (los nacionales, los de la comunidad y los locales del perfil).
//!
//! Reglas de cómputo (Ley 39/2015, aplicable supletoriamente a los procedimientos tributarios
//! por su disp. adicional 1.ª.2.a y el art. 7.2 LGT):
//! - **Días** (art. 30.2): se entienden hábiles salvo que la norma diga naturales; no cuentan
//!   sábados, domingos ni festivos (nacionales, autonómicos y locales del domicilio, art. 30.6),
//!   y se cuentan desde el día siguiente a la notificación (art. 30.3).
//! - **Meses** (art. 30.4): de fecha a fecha, desde el día siguiente a la notificación, y el
//!   plazo acaba el mismo día del mes de vencimiento en que se notificó; si ese mes no tiene
//!   ese día, el último del mes.
//! - Si el último día es inhábil, se prorroga al primer día hábil siguiente (art. 30.5).
//! - Una notificación electrónica sin abrir se entiende rechazada (y practicada) a los diez
//!   días naturales de la puesta a disposición (art. 43.2).

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::HolidayCalendar;
use taxpain_core::calendar::rules::add_months;
use taxpain_core::money::{Money, Rate};
use time::{Date, Duration, Month};

/// Qué es lo notificado: decide los plazos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    /// Requerimiento de información o de documentación.
    #[default]
    Requerimiento,
    /// Propuesta de liquidación (con trámite de alegaciones).
    PropuestaLiquidacion,
    /// Trámite de audiencia.
    Audiencia,
    /// Propuesta de imposición de sanción.
    PropuestaSancion,
    /// Liquidación (provisional o definitiva).
    Liquidacion,
    /// Acuerdo de imposición de sanción.
    AcuerdoSancionador,
    /// Providencia de apremio.
    ProvidenciaApremio,
    /// Diligencia de embargo.
    DiligenciaEmbargo,
    Otro,
}

impl Tipo {
    pub fn code(self) -> &'static str {
        match self {
            Tipo::Requerimiento => "requerimiento",
            Tipo::PropuestaLiquidacion => "propuesta_liquidacion",
            Tipo::Audiencia => "audiencia",
            Tipo::PropuestaSancion => "propuesta_sancion",
            Tipo::Liquidacion => "liquidacion",
            Tipo::AcuerdoSancionador => "acuerdo_sancionador",
            Tipo::ProvidenciaApremio => "providencia_apremio",
            Tipo::DiligenciaEmbargo => "diligencia_embargo",
            Tipo::Otro => "otro",
        }
    }

    pub fn from_code(s: &str) -> Option<Self> {
        Some(match s {
            "requerimiento" => Tipo::Requerimiento,
            "propuesta_liquidacion" => Tipo::PropuestaLiquidacion,
            "audiencia" => Tipo::Audiencia,
            "propuesta_sancion" => Tipo::PropuestaSancion,
            "liquidacion" => Tipo::Liquidacion,
            "acuerdo_sancionador" => Tipo::AcuerdoSancionador,
            "providencia_apremio" => Tipo::ProvidenciaApremio,
            "diligencia_embargo" => Tipo::DiligenciaEmbargo,
            "otro" => Tipo::Otro,
            _ => return None,
        })
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Tipo::Requerimiento => "Requerimiento de información",
            Tipo::PropuestaLiquidacion => "Propuesta de liquidación",
            Tipo::Audiencia => "Trámite de audiencia",
            Tipo::PropuestaSancion => "Propuesta de sanción",
            Tipo::Liquidacion => "Liquidación",
            Tipo::AcuerdoSancionador => "Acuerdo sancionador",
            Tipo::ProvidenciaApremio => "Providencia de apremio",
            Tipo::DiligenciaEmbargo => "Diligencia de embargo",
            Tipo::Otro => "Otra notificación",
        }
    }

    /// Acto que se recurre (reposición o reclamación económico-administrativa).
    pub fn recurrible(self) -> bool {
        matches!(
            self,
            Tipo::Liquidacion
                | Tipo::AcuerdoSancionador
                | Tipo::ProvidenciaApremio
                | Tipo::DiligenciaEmbargo
        )
    }

    /// Trámite en el que se contesta o se alega dentro del procedimiento (días hábiles).
    pub fn de_tramite(self) -> bool {
        matches!(
            self,
            Tipo::Requerimiento
                | Tipo::PropuestaLiquidacion
                | Tipo::Audiencia
                | Tipo::PropuestaSancion
        )
    }
}

/// Plazo que corre.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plazo {
    /// `contestar`, `alegar`, `pedir_ampliacion`, `reposicion`, `reclamacion`, `pago`.
    pub clave: &'static str,
    pub titulo: String,
    /// Último día (ya trasladado si caía en inhábil).
    pub vence: Date,
    /// Cómo se ha contado.
    pub computo: String,
    pub norma: &'static str,
    /// Fijado a mano (lo dice el documento o una ampliación concedida).
    pub manual: bool,
}

/// Día en que se entiende practicada una notificación electrónica no abierta: a los diez días
/// naturales de la puesta a disposición (art. 43.2 Ley 39/2015). Ver
/// [`crate::notices::deemed_notified_on`], que usa la misma regla.
pub fn notificacion_presunta(puesto: Date) -> Date {
    puesto + Duration::days(crate::notices::ACCESS_DAYS)
}

/// Último día de un plazo de `n` días hábiles contados desde el siguiente a `desde` (art. 30.2
/// y 30.3 Ley 39/2015). El último ya es hábil por construcción.
pub fn dias_habiles(cal: &HolidayCalendar, desde: Date, n: u32) -> Date {
    let mut d = desde;
    let mut k = 0;
    while k < n {
        d += Duration::days(1);
        if cal.is_business_day(d) {
            k += 1;
        }
    }
    d
}

/// Último día de un plazo de `n` meses de fecha a fecha desde la notificación `desde` (art.
/// 30.4 Ley 39/2015: el mismo día del mes de vencimiento o, si no lo tiene, el último del mes),
/// prorrogado al primer hábil si cae en inhábil (art. 30.5).
pub fn meses(cal: &HolidayCalendar, desde: Date, n: i32) -> Date {
    cal.roll_forward(add_months(desde, n))
}

fn dia(y: i32, m: Month, d: u8) -> Date {
    Date::from_calendar_date(y, m, d).unwrap_or(Date::MIN)
}

/// Fin del periodo voluntario de pago de una deuda liquidada por la Administración (art. 62.2
/// LGT): notificada entre el 1 y el 15 del mes, hasta el 20 del mes siguiente; entre el 16 y el
/// último, hasta el 5 del segundo mes siguiente; si es inhábil, el primer hábil siguiente.
pub fn pago_voluntario(cal: &HolidayCalendar, notificado: Date) -> Date {
    let base = dia(notificado.year(), notificado.month(), 1);
    let d = if notificado.day() <= 15 {
        let m = add_months(base, 1);
        dia(m.year(), m.month(), 20)
    } else {
        let m = add_months(base, 2);
        dia(m.year(), m.month(), 5)
    };
    cal.roll_forward(d)
}

/// Plazo para pagar una providencia de apremio (art. 62.5 LGT): notificada entre el 1 y el 15,
/// hasta el 20 de ese mes; entre el 16 y el último, hasta el 5 del mes siguiente; si es
/// inhábil, el primer hábil siguiente.
pub fn pago_apremio(cal: &HolidayCalendar, notificado: Date) -> Date {
    let d = if notificado.day() <= 15 {
        dia(notificado.year(), notificado.month(), 20)
    } else {
        let m = add_months(dia(notificado.year(), notificado.month(), 1), 1);
        dia(m.year(), m.month(), 5)
    };
    cal.roll_forward(d)
}

/// Plazo general para atender un requerimiento: diez días hábiles. `TODO(VERIFY)`: el art.
/// 29.2.g LGT y el RGAT (RD 1065/2007) dicen «no inferior a diez días»; manda el que diga el
/// propio requerimiento (por eso se puede teclear).
pub const DIAS_REQUERIMIENTO: u32 = 10;
/// Alegaciones a una propuesta de liquidación o trámite de audiencia: entre diez y quince
/// días hábiles; se toma el mínimo si el documento no dice otro. `TODO(VERIFY)`: art. 99.8 LGT
/// y art. 96 RGAT (gestión); en inspección, art. 157 LGT (15 días en las actas de
/// disconformidad).
pub const DIAS_ALEGACIONES: u32 = 10;
/// Alegaciones a la propuesta de sanción: quince días (art. 210.4 LGT; `TODO(VERIFY)`).
pub const DIAS_ALEGACIONES_SANCION: u32 = 15;
/// La ampliación del plazo se pide como tarde tres días antes de que acabe y no puede pasar
/// de la mitad del plazo (art. 91 RGAT; `TODO(VERIFY)` si los tres días son naturales).
pub const DIAS_ANTES_AMPLIACION: i64 = 3;

/// Plazos que abre la notificación del `tipo` el día `notificado`. `plazo_dias`: el que dice el
/// documento (días hábiles); `vence_manual`: un último día fijado a mano, que sustituye al del
/// trámite (contestar o alegar).
pub fn plazos(
    cal: &HolidayCalendar,
    tipo: Tipo,
    notificado: Date,
    plazo_dias: Option<u32>,
    vence_manual: Option<Date>,
) -> Vec<Plazo> {
    let mut out = Vec::new();
    let tramite = |clave: &'static str, titulo: &str, defecto: u32, norma: &'static str| {
        let n = plazo_dias.unwrap_or(defecto);
        match vence_manual {
            Some(v) => Plazo {
                clave,
                titulo: titulo.to_owned(),
                vence: v,
                computo: "Fijado a mano (lo que dice el documento o el nuevo plazo concedido)"
                    .into(),
                norma,
                manual: true,
            },
            None => Plazo {
                clave,
                titulo: titulo.to_owned(),
                vence: dias_habiles(cal, notificado, n),
                computo: format!(
                    "{n} días hábiles desde el día siguiente a la notificación, sin sábados, \
                     domingos ni festivos{}",
                    if plazo_dias.is_some() {
                        " (el plazo que da el documento)"
                    } else {
                        " (el plazo general: si el documento da otro, tecléalo)"
                    }
                ),
                norma,
                manual: false,
            },
        }
    };
    match tipo {
        Tipo::Requerimiento | Tipo::PropuestaLiquidacion | Tipo::Audiencia => {
            let (clave, titulo, defecto, norma) = if tipo == Tipo::Requerimiento {
                (
                    "contestar",
                    "Contestar el requerimiento",
                    DIAS_REQUERIMIENTO,
                    "Art. 29.2.g LGT; RGAT (RD 1065/2007); arts. 30 y 31 Ley 39/2015. \
                     TODO(VERIFY)",
                )
            } else {
                (
                    "alegar",
                    "Presentar alegaciones",
                    DIAS_ALEGACIONES,
                    "Arts. 34.1.m, 99.8 y 157 LGT; art. 96 RGAT. TODO(VERIFY)",
                )
            };
            let p = tramite(clave, titulo, defecto, norma);
            if !p.manual {
                // La ampliación: como tarde tres días antes del final y hasta la mitad del plazo.
                let n = plazo_dias.unwrap_or(defecto);
                let pedir = p.vence - Duration::days(DIAS_ANTES_AMPLIACION);
                if pedir > notificado {
                    out.push(Plazo {
                        clave: "pedir_ampliacion",
                        titulo: format!(
                            "Pedir la ampliación del plazo (hasta {} días hábiles más: \
                             terminaría el {})",
                            n / 2,
                            fecha(dias_habiles(cal, notificado, n + n / 2))
                        ),
                        vence: pedir,
                        computo: "Tres días antes de que acabe el plazo; la ampliación no \
                                  puede pasar de la mitad del plazo concedido"
                            .into(),
                        norma: "Art. 91 RGAT (RD 1065/2007). TODO(VERIFY)",
                        manual: false,
                    });
                }
            }
            out.insert(0, p);
        }
        Tipo::PropuestaSancion => out.push(tramite(
            "alegar",
            "Presentar alegaciones a la propuesta de sanción",
            DIAS_ALEGACIONES_SANCION,
            "Art. 210.4 LGT; art. 23 RGRST (RD 2063/2004). TODO(VERIFY)",
        )),
        Tipo::Liquidacion | Tipo::AcuerdoSancionador => {
            out.push(Plazo {
                clave: "pago",
                titulo: "Pagar en periodo voluntario (o pedir antes el aplazamiento)".into(),
                vence: pago_voluntario(cal, notificado),
                computo: "Notificada del 1 al 15: hasta el 20 del mes siguiente; del 16 al \
                          último día: hasta el 5 del segundo mes siguiente"
                    .into(),
                norma: "Art. 62.2 LGT; art. 65.5 LGT (la solicitud de aplazamiento en \
                        periodo voluntario impide el inicio del periodo ejecutivo)",
                manual: false,
            });
        }
        Tipo::ProvidenciaApremio => {
            out.push(Plazo {
                clave: "pago",
                titulo: "Pagar con el recargo de apremio reducido (10 %)".into(),
                vence: pago_apremio(cal, notificado),
                computo: "Notificada del 1 al 15: hasta el 20 de ese mes; del 16 al último \
                          día: hasta el 5 del mes siguiente"
                    .into(),
                norma: "Arts. 28.3 y 62.5 LGT",
                manual: false,
            });
        }
        Tipo::DiligenciaEmbargo => {}
        Tipo::Otro => {
            if plazo_dias.is_some() || vence_manual.is_some() {
                out.push(tramite(
                    "contestar",
                    "Contestar",
                    DIAS_REQUERIMIENTO,
                    "Arts. 30 y 31 Ley 39/2015",
                ));
            }
        }
    }
    if tipo.recurrible() {
        let un_mes = meses(cal, notificado, 1);
        let computo = format!(
            "Un mes de fecha a fecha desde el día siguiente a la notificación (del {} al {}){}",
            fecha(notificado + Duration::days(1)),
            fecha(add_months(notificado, 1)),
            if un_mes == add_months(notificado, 1) {
                String::new()
            } else {
                ", trasladado al primer día hábil".to_owned()
            }
        );
        out.push(Plazo {
            clave: "reposicion",
            titulo: "Recurso de reposición (potestativo)".into(),
            vence: un_mes,
            computo: computo.clone(),
            norma: "Arts. 222 y 223.1 LGT. TODO(VERIFY)",
            manual: false,
        });
        out.push(Plazo {
            clave: "reclamacion",
            titulo: "Reclamación económico-administrativa (si no se recurre en reposición)".into(),
            vence: un_mes,
            computo,
            norma: "Art. 235.1 LGT. TODO(VERIFY)",
            manual: false,
        });
    }
    out
}

fn fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Lo que costaría pagar una deuda en cada momento del periodo ejecutivo (art. 28 LGT).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Escenario {
    /// Hasta cuándo vale (incluido); `None`: desde entonces en adelante.
    pub hasta: Option<Date>,
    pub texto: String,
    /// Porcentaje del recargo (0, 5, 10 o 20).
    pub porcentaje: u8,
    pub recargo: Money,
    /// Intereses de demora calculados hasta hoy (sólo con el recargo del 20 %).
    pub intereses: Money,
    pub total: Money,
    /// Es lo que toca si se paga hoy.
    pub hoy: bool,
}

/// Recargos del periodo ejecutivo (art. 28 LGT): 5 % si se paga entera la deuda antes de que se
/// notifique la providencia de apremio (ejecutivo); 10 % si se paga entera con lo apremiado en
/// el plazo del art. 62.5 (apremio reducido); 20 % después (apremio ordinario), y sólo éste con
/// intereses de demora (art. 28.5 LGT), desde el día siguiente al fin del periodo voluntario
/// (art. 26.2.a LGT; los calcula [`crate::rectificativa::intereses`]).
///
/// `apremio`: notificación de la providencia y fin de su plazo de pago, si ya se notificó.
pub fn recargo_ejecutivo(
    deuda: Money,
    fin_voluntario: Date,
    apremio: Option<(Date, Date)>,
    hoy: Date,
) -> Vec<Escenario> {
    if deuda <= Money::ZERO {
        return Vec::new();
    }
    let con = |pct: u8| deuda.apply_rate(Rate::percent(i32::from(pct)));
    let mut out = Vec::new();
    if hoy <= fin_voluntario {
        out.push(Escenario {
            hasta: Some(fin_voluntario),
            texto: format!(
                "Pagando hasta el {} (periodo voluntario): sin recargo",
                fecha(fin_voluntario)
            ),
            porcentaje: 0,
            recargo: Money::ZERO,
            intereses: Money::ZERO,
            total: deuda,
            hoy: false,
        });
    }
    match apremio {
        None => out.push(Escenario {
            hasta: None,
            texto: format!(
                "Pagando después del {} y antes de que te notifiquen la providencia de apremio: \
                 recargo ejecutivo del 5 % (art. 28.2 LGT)",
                fecha(fin_voluntario)
            ),
            porcentaje: 5,
            recargo: con(5),
            intereses: Money::ZERO,
            total: deuda + con(5),
            hoy: false,
        }),
        Some((_, fin_apremio)) => {
            out.push(Escenario {
                hasta: Some(fin_apremio),
                texto: format!(
                    "Pagando hasta el {} (plazo de la providencia): recargo de apremio \
                     reducido del 10 % (art. 28.3 LGT)",
                    fecha(fin_apremio)
                ),
                porcentaje: 10,
                recargo: con(10),
                intereses: Money::ZERO,
                total: deuda + con(10),
                hoy: false,
            });
            let desde = fin_voluntario + Duration::days(1);
            let intereses = if hoy > fin_apremio {
                crate::rectificativa::intereses(deuda, desde, hoy)
            } else {
                Money::ZERO
            };
            out.push(Escenario {
                hasta: None,
                texto: format!(
                    "Pagando después del {}: recargo de apremio ordinario del 20 % más los \
                     intereses de demora desde el {} (arts. 28.4 y 28.5 LGT){}",
                    fecha(fin_apremio),
                    fecha(desde),
                    if hoy > fin_apremio {
                        " (calculados hasta hoy)"
                    } else {
                        ""
                    }
                ),
                porcentaje: 20,
                recargo: con(20),
                intereses,
                total: deuda + con(20) + intereses,
                hoy: false,
            });
        }
    }
    // El que toca hoy: el primero cuyo «hasta» no ha pasado.
    if let Some(e) = out.iter_mut().find(|e| e.hasta.is_none_or(|h| hoy <= h)) {
        e.hoy = true;
    }
    out
}

/// Lo que se pagaría de una sanción con las reducciones del art. 188 LGT.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reducciones {
    /// Sanción sin reducir.
    pub sancion: Money,
    /// Con la reducción del 30 % por conformidad con la regularización (art. 188.1.b LGT).
    pub conformidad: Money,
    /// Sólo con la reducción del 40 % por pronto pago (art. 188.3 LGT).
    pub pronto_pago: Money,
    /// Con las dos: el 40 % se aplica sobre lo que queda tras la de conformidad (art. 188.3).
    pub ambas: Money,
    pub notas: Vec<String>,
}

/// Reducción por conformidad: 30 % (art. 188.1.b LGT). `TODO(VERIFY)`: sólo para las sanciones
/// de los arts. 191 a 197 LGT; en actas con acuerdo es el 65 % (art. 188.1.a).
pub const REDUCCION_CONFORMIDAD: i32 = 30;
/// Reducción por pronto pago: 40 % desde la Ley 11/2021 (antes 25 %) si se paga en el plazo
/// del art. 62.2 LGT sin recurrir la sanción ni la liquidación (art. 188.3 LGT; `TODO(VERIFY)`).
pub const REDUCCION_PRONTO_PAGO: i32 = 40;

pub fn reducciones(sancion: Money) -> Reducciones {
    let menos = |m: Money, pct: i32| m - m.apply_rate(Rate::percent(pct));
    let conformidad = menos(sancion, REDUCCION_CONFORMIDAD);
    Reducciones {
        sancion,
        conformidad,
        pronto_pago: menos(sancion, REDUCCION_PRONTO_PAGO),
        ambas: menos(conformidad, REDUCCION_PRONTO_PAGO),
        notas: vec![
            "Conformidad (30 %): si das la conformidad a la propuesta de regularización; se \
             exige lo reducido si después recurres la regularización (art. 188.2 LGT)."
                .into(),
            "Pronto pago (40 %): si pagas lo que queda en el plazo de pago voluntario (art. \
             62.2 LGT) o en el del aplazamiento con garantía concedido, y no recurres ni la \
             sanción ni la liquidación (art. 188.3 LGT)."
                .into(),
            "Recurrir la sanción la deja suspendida sin garantía hasta que sea firme en vía \
             administrativa (art. 212.3 LGT), pero se pierde la reducción por pronto pago."
                .into(),
            "TODO(VERIFY): la conformidad sólo reduce las sanciones de los arts. 191 a 197 LGT; \
             en un acta con acuerdo la reducción es del 65 % (art. 188.1.a LGT)."
                .into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taxpain_core::calendar::calendar_for;
    use time::macros::date;

    fn madrid_2026() -> HolidayCalendar {
        calendar_for(2026, Some(taxpain_core::calendar::Ccaa::Madrid), &[]).0
    }

    #[test]
    fn diez_dias_habiles_saltan_fines_de_semana_y_festivos() {
        let cal = calendar_for(2026, None, &[]).0;
        // Notificado el lunes 23/11/2026: 24 a 27 (4), 30/11 (5), 1 a 4/12 (9) y lunes 7/12 (10).
        assert_eq!(
            dias_habiles(&cal, date!(2026 - 11 - 23), 10),
            date!(2026 - 12 - 07)
        );
        // En Madrid el 7/12 es festivo (el 6 cae en domingo) y el 8 también: miércoles 9/12.
        assert_eq!(
            dias_habiles(&madrid_2026(), date!(2026 - 11 - 23), 10),
            date!(2026 - 12 - 09)
        );
        // Notificado el viernes 4/12/2026: el 8/12 es festivo; 7 (1), 9, 10, 11 (4), 14-18 (9),
        // 21 (10).
        assert_eq!(
            dias_habiles(&cal, date!(2026 - 12 - 04), 10),
            date!(2026 - 12 - 21)
        );
        // Cruza el año: notificado el 18/12/2026; 21 a 24 (4), el 25 festivo, 28 a 31 (8), el
        // 1/1/2027 festivo, lunes 4/1 (9) y martes 5/1 (10).
        assert_eq!(
            dias_habiles(&cal, date!(2026 - 12 - 18), 10),
            date!(2027 - 01 - 05)
        );
    }

    #[test]
    fn festivo_autonomico_y_local_cuentan() {
        // Viernes Santo (3/4/2026) es nacional; Jueves Santo (2/4) lo es en Madrid.
        let nacional = calendar_for(2026, None, &[]).0;
        let madrid = madrid_2026();
        // Notificado el lunes 30/3/2026, tres días hábiles: 31, 1 y 2/4; en Madrid el 2 y el 3
        // no cuentan y el tercero es el lunes 6/4.
        assert_eq!(
            dias_habiles(&nacional, date!(2026 - 03 - 30), 3),
            date!(2026 - 04 - 02)
        );
        assert_eq!(
            dias_habiles(&madrid, date!(2026 - 03 - 30), 3),
            date!(2026 - 04 - 06)
        );
        // Un festivo local (inventado) también salta.
        let local = calendar_for(2026, None, &[date!(2026 - 03 - 31)]).0;
        assert_eq!(
            dias_habiles(&local, date!(2026 - 03 - 30), 1),
            date!(2026 - 04 - 01)
        );
    }

    #[test]
    fn un_mes_de_fecha_a_fecha() {
        let cal = calendar_for(2026, None, &[]).0;
        // 15/10/2026 → 15/11/2026, domingo → lunes 16/11.
        assert_eq!(meses(&cal, date!(2026 - 10 - 15), 1), date!(2026 - 11 - 16));
        // 13/10/2026 → 13/11/2026, viernes: no se mueve.
        assert_eq!(meses(&cal, date!(2026 - 10 - 13), 1), date!(2026 - 11 - 13));
        // 31/1/2026 → febrero no tiene 31: el 28 (sábado) → lunes 2/3.
        assert_eq!(meses(&cal, date!(2026 - 01 - 31), 1), date!(2026 - 03 - 02));
        // 8/11/2026 → 8/12/2026, festivo (Inmaculada) → miércoles 9/12.
        assert_eq!(meses(&cal, date!(2026 - 11 - 08), 1), date!(2026 - 12 - 09));
        // 1/12/2026 → 1/1/2027, festivo → lunes 4/1/2027 (el 2 y el 3 son fin de semana).
        assert_eq!(meses(&cal, date!(2026 - 12 - 01), 1), date!(2027 - 01 - 04));
    }

    #[test]
    fn periodo_voluntario_y_apremio() {
        let cal = calendar_for(2026, None, &[]).0;
        // Liquidación notificada el 10/11/2026: hasta el 20/12, domingo → lunes 21/12.
        assert_eq!(
            pago_voluntario(&cal, date!(2026 - 11 - 10)),
            date!(2026 - 12 - 21)
        );
        // Notificada el 16/11/2026: hasta el 5/1/2027 (martes).
        assert_eq!(
            pago_voluntario(&cal, date!(2026 - 11 - 16)),
            date!(2027 - 01 - 05)
        );
        // Providencia notificada el 2/11/2026: hasta el 20/11 (viernes).
        assert_eq!(
            pago_apremio(&cal, date!(2026 - 11 - 02)),
            date!(2026 - 11 - 20)
        );
        // Notificada el 20/11/2026: hasta el 5/12 (sábado) → lunes 7/12.
        assert_eq!(
            pago_apremio(&cal, date!(2026 - 11 - 20)),
            date!(2026 - 12 - 07)
        );
    }

    #[test]
    fn notificacion_sin_abrir_a_los_diez_dias_naturales() {
        assert_eq!(
            notificacion_presunta(date!(2026 - 07 - 28)),
            date!(2026 - 08 - 07)
        );
    }

    #[test]
    fn plazos_de_un_requerimiento_con_ampliacion() {
        let cal = calendar_for(2026, None, &[]).0;
        let p = plazos(&cal, Tipo::Requerimiento, date!(2026 - 11 - 23), None, None);
        assert_eq!(p[0].clave, "contestar");
        assert_eq!(p[0].vence, date!(2026 - 12 - 07));
        assert_eq!(p[1].clave, "pedir_ampliacion");
        assert_eq!(p[1].vence, date!(2026 - 12 - 04));
        // Cinco días más como mucho: 9, 10, 11, 14, 15/12.
        assert!(p[1].titulo.contains("15/12/2026"), "{}", p[1].titulo);
        // Con el plazo del documento (15 días) y con uno fijado a mano.
        let p = plazos(
            &cal,
            Tipo::Requerimiento,
            date!(2026 - 11 - 23),
            Some(15),
            None,
        );
        assert_eq!(p[0].vence, date!(2026 - 12 - 15));
        let p = plazos(
            &cal,
            Tipo::Requerimiento,
            date!(2026 - 11 - 23),
            None,
            Some(date!(2026 - 12 - 22)),
        );
        assert!(p[0].manual && p[0].vence == date!(2026 - 12 - 22) && p.len() == 1);
    }

    #[test]
    fn plazos_de_una_liquidacion_y_de_una_sancion() {
        let cal = calendar_for(2026, None, &[]).0;
        let p = plazos(&cal, Tipo::Liquidacion, date!(2026 - 10 - 15), None, None);
        let claves: Vec<_> = p.iter().map(|x| x.clave).collect();
        assert_eq!(claves, ["pago", "reposicion", "reclamacion"]);
        assert_eq!(p[0].vence, date!(2026 - 11 - 20));
        assert_eq!(p[1].vence, date!(2026 - 11 - 16));
        assert!(p[1].computo.contains("trasladado"), "{}", p[1].computo);
        let p = plazos(
            &cal,
            Tipo::PropuestaSancion,
            date!(2026 - 11 - 23),
            None,
            None,
        );
        // 15 días hábiles: 24-27 (4), 30/11-4/12 (9), 7 (10), 9-11 (13), 14, 15 (15).
        assert_eq!(p[0].vence, date!(2026 - 12 - 15));
        assert!(
            plazos(
                &cal,
                Tipo::DiligenciaEmbargo,
                date!(2026 - 11 - 23),
                None,
                None
            )
            .iter()
            .all(|x| x.clave != "pago")
        );
    }

    #[test]
    fn recargos_del_periodo_ejecutivo() {
        let deuda = Money::from_euros(1_000, 0);
        // Vencido el voluntario sin providencia: 5 %.
        let e = recargo_ejecutivo(deuda, date!(2026 - 10 - 20), None, date!(2026 - 11 - 16));
        assert_eq!(e.len(), 1);
        assert!(e[0].hoy && e[0].porcentaje == 5);
        assert_eq!(e[0].total, Money::from_euros(1_050, 0));
        // Aún en voluntario: sin recargo hoy.
        let e = recargo_ejecutivo(deuda, date!(2026 - 11 - 20), None, date!(2026 - 11 - 16));
        assert!(e[0].hoy && e[0].porcentaje == 0);
        // Providencia notificada el 2/11, plazo hasta el 20/11: 10 % hoy.
        let ap = Some((date!(2026 - 11 - 02), date!(2026 - 11 - 20)));
        let e = recargo_ejecutivo(deuda, date!(2026 - 06 - 22), ap, date!(2026 - 11 - 16));
        assert_eq!(e.len(), 2);
        assert!(e[0].hoy && e[0].porcentaje == 10 && e[0].recargo == Money::from_euros(100, 0));
        assert_eq!(e[1].intereses, Money::ZERO);
        // Pasado el plazo de la providencia: 20 % más intereses desde el 23/6.
        let e = recargo_ejecutivo(deuda, date!(2026 - 06 - 22), ap, date!(2026 - 12 - 01));
        assert!(e[1].hoy && e[1].porcentaje == 20);
        assert!(e[1].intereses > Money::ZERO);
        assert_eq!(
            e[1].intereses,
            crate::rectificativa::intereses(deuda, date!(2026 - 06 - 23), date!(2026 - 12 - 01))
        );
        assert_eq!(
            e[1].total,
            deuda + Money::from_euros(200, 0) + e[1].intereses
        );
    }

    #[test]
    fn reducciones_de_la_sancion() {
        let r = reducciones(Money::from_euros(1_000, 0));
        assert_eq!(r.conformidad, Money::from_euros(700, 0));
        assert_eq!(r.pronto_pago, Money::from_euros(600, 0));
        // 1.000 × 0,70 × 0,60.
        assert_eq!(r.ambas, Money::from_euros(420, 0));
    }
}
