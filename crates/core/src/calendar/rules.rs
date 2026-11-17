//! Obligaciones según el perfil censal y ventanas de presentación de cada modelo.
//!
//! Cada regla lleva su referencia en `rule_ref`. Fuente de verificación: calendario del
//! contribuyente AEAT y `docs/CALENDAR_RULES.md`; las fechas oficiales de 2026 están en
//! `fixtures/calendar/aeat-2026.json` y los tests de `tests/calendar_aeat_2026.rs` las comparan.

use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month};

use super::business::HolidayCalendar;
use super::holidays::Ccaa;
use super::official;

/// Modelos y obligaciones con plazo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Model {
    M303,
    M130,
    M131,
    M111,
    M115,
    M123,
    M349,
    M390,
    M190,
    M180,
    M193,
    /// Retenciones a no residentes sin establecimiento permanente (IRNR).
    M216,
    /// Resumen anual de retenciones a no residentes.
    M296,
    M347,
    M184,
    M200,
    M202,
    M100,
    DepositoCuentas,
    LegalizacionLibros,
    /// Bienes y derechos en el extranjero (cuentas, valores, inmuebles).
    M720,
    /// Monedas virtuales custodiadas en el extranjero.
    M721,
    /// Operaciones vinculadas y con paraísos fiscales.
    M232,
    /// Renta: ingreso del segundo plazo (40 %) si se fraccionó el pago.
    M100SegundoPlazo,
    /// IAE: comunicación del importe neto de la cifra de negocios.
    M848,
    /// IAE: alta al dejar de estar exento.
    M840,
    /// IVA de la ventanilla única (OSS/IOSS): ventas a distancia a consumidores de la UE.
    M369,
    /// Censo (036/037): REDEME y SII para el año siguiente (noviembre).
    OpcionesNoviembre,
    /// Censo (036/037): renuncias y opciones de diciembre para el año siguiente.
    OpcionesDiciembre,
    /// Censo (036): optar por el 202 por la base del periodo (art. 40.3 LIS) o renunciar, con
    /// efectos en el período impositivo que empieza (febrero en un ejercicio natural).
    Opcion202Base,
    /// Opción por la prorrata especial del IVA (en el 303 del último periodo).
    ProrrataEspecial,
    /// Seguridad Social: cuota mensual de autónomos (RETA).
    CuotaAutonomos,
    /// Seguridad Social: último día para pedir el cambio de la base de cotización (o del
    /// rendimiento previsto) del RETA con efectos el mes siguiente.
    CambioBaseReta,
    /// Seguridad Social: seguros sociales de los trabajadores (RLC/RNT).
    SegurosSociales,
}

impl Model {
    pub fn code(self) -> &'static str {
        match self {
            Model::M303 => "303",
            Model::M130 => "130",
            Model::M131 => "131",
            Model::M111 => "111",
            Model::M115 => "115",
            Model::M123 => "123",
            Model::M349 => "349",
            Model::M390 => "390",
            Model::M190 => "190",
            Model::M180 => "180",
            Model::M193 => "193",
            Model::M216 => "216",
            Model::M296 => "296",
            Model::M347 => "347",
            Model::M184 => "184",
            Model::M200 => "200",
            Model::M202 => "202",
            Model::M100 => "100",
            Model::DepositoCuentas => "deposito-cuentas",
            Model::LegalizacionLibros => "legalizacion-libros",
            Model::M720 => "720",
            Model::M721 => "721",
            Model::M232 => "232",
            // Mismo modelo que la Renta; el periodo («2P») lo distingue.
            Model::M100SegundoPlazo => "100",
            Model::M848 => "848",
            Model::M840 => "840",
            Model::M369 => "369",
            Model::OpcionesNoviembre => "036-noviembre",
            Model::OpcionesDiciembre => "036-diciembre",
            Model::Opcion202Base => "036-202",
            Model::ProrrataEspecial => "prorrata-especial",
            Model::CuotaAutonomos => "cuota-autonomos",
            Model::CambioBaseReta => "cambio-base-reta",
            Model::SegurosSociales => "seguros-sociales",
        }
    }

    /// Título corto para listas, avisos y el .ics: «Modelo 303» para los modelos y el nombre
    /// del trámite para lo demás.
    pub fn title(self) -> String {
        match self {
            Model::DepositoCuentas => "Depósito de cuentas".into(),
            Model::LegalizacionLibros => "Legalización de libros".into(),
            Model::OpcionesNoviembre | Model::OpcionesDiciembre => "Modelo 036/037".into(),
            Model::Opcion202Base => "Modelo 036".into(),
            Model::ProrrataEspecial => "Prorrata especial del IVA".into(),
            Model::CuotaAutonomos => "Cuota de autónomos".into(),
            Model::CambioBaseReta => "Cambio de base del RETA".into(),
            Model::SegurosSociales => "Seguros sociales".into(),
            m => format!("Modelo {}", m.code()),
        }
    }

    /// Explicación en lenguaje llano para la UI y las notificaciones.
    pub fn description(self) -> &'static str {
        match self {
            Model::M303 => "IVA: declaración del periodo",
            Model::M130 => "IRPF: pago fraccionado (estimación directa)",
            Model::M131 => "IRPF: pago fraccionado (módulos)",
            Model::M111 => "Retenciones de trabajadores y profesionales",
            Model::M115 => "Retenciones del alquiler de locales",
            Model::M123 => "Retenciones de capital mobiliario",
            Model::M349 => "Operaciones intracomunitarias",
            Model::M390 => "IVA: resumen anual",
            Model::M190 => "Resumen anual de retenciones de trabajo y profesionales",
            Model::M180 => "Resumen anual de retenciones de alquileres",
            Model::M193 => "Resumen anual de retenciones de capital mobiliario",
            Model::M216 => "Retenciones de rentas pagadas a no residentes",
            Model::M296 => "Resumen anual de retenciones a no residentes",
            Model::M347 => "Operaciones con terceros de más de 3.005,06 €",
            Model::M184 => "Entidades en régimen de atribución de rentas",
            Model::M200 => "Impuesto sobre Sociedades",
            Model::M202 => "Impuesto sobre Sociedades: pago fraccionado",
            Model::M100 => "Declaración de la Renta",
            Model::DepositoCuentas => "Depósito de cuentas anuales en el Registro Mercantil",
            Model::LegalizacionLibros => "Legalización de libros en el Registro Mercantil",
            Model::M720 => "Cuentas, valores e inmuebles en el extranjero (más de 50.000 €)",
            Model::M721 => "Criptomonedas guardadas en el extranjero (más de 50.000 €)",
            Model::M232 => {
                "Operaciones vinculadas (socios, administradores, grupo) y con \
                            paraísos fiscales"
            }
            Model::M100SegundoPlazo => "Renta: pago del segundo plazo (40 %)",
            Model::M848 => {
                "IAE: comunicación de la cifra de negocios (sólo si no la hiciste constar en \
                 el 184 ni en el 200)"
            }
            Model::M840 => "IAE: alta al dejar de estar exento (cifra de negocios de 1 M€ o más)",
            Model::M369 => {
                "IVA de ventas a consumidores de otros países de la UE (ventanilla \
                            única OSS/IOSS); el plazo no se amplía si acaba en festivo"
            }
            Model::OpcionesNoviembre => {
                "Censo: inscribirte o darte de baja en el REDEME y renunciar al SII \
                                         voluntario para el año siguiente (optar por él se puede \
                                         en cualquier momento del año)"
            }
            Model::OpcionesDiciembre => {
                "Censo: renunciar a módulos o a la estimación directa \
                                         simplificada, al régimen simplificado del IVA o al \
                                         criterio de caja (u optar por él) para el año siguiente"
            }
            Model::Opcion202Base => {
                "Censo: optar por calcular el 202 por la base imponible del periodo (art. 40.3 \
                 LIS) o renunciar a ello, para el ejercicio que empieza. Compara antes las dos \
                 modalidades en Modelos → 202"
            }
            Model::ProrrataEspecial => {
                "Si haces operaciones exentas: la opción por la prorrata \
                                        especial se marca en el 303 del último periodo"
            }
            Model::CuotaAutonomos => {
                "Seguridad Social: cuota de autónomos del mes (se carga \
                                      el último día hábil)"
            }
            Model::CambioBaseReta => {
                "Seguridad Social: último día para pedir el cambio de la base de cotización \
                 o del rendimiento neto previsto del RETA (vale desde el día 1 del mes \
                 siguiente)"
            }
            Model::SegurosSociales => {
                "Seguridad Social: cotizaciones de los trabajadores del \
                                       mes anterior"
            }
        }
    }

    /// Días antes del vencimiento en que avisar, si no son los de siempre: la cuota de
    /// autónomos se carga sola cada mes y basta un aviso para tener saldo.
    pub fn reminder_offsets(self) -> Option<&'static [i64]> {
        match self {
            Model::CuotaAutonomos => Some(&[2]),
            Model::CambioBaseReta => Some(&[10, 2]),
            Model::SegurosSociales => Some(&[7, 2, 0]),
            _ => None,
        }
    }

    /// Autoliquidación con ingreso: admite domiciliación bancaria.
    pub fn is_self_assessment(self) -> bool {
        matches!(
            self,
            Model::M303
                | Model::M130
                | Model::M131
                | Model::M111
                | Model::M115
                | Model::M123
                | Model::M216
                | Model::M200
                | Model::M202
                | Model::M100
        )
    }
}

/// Periodo de liquidación.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Period {
    Quarter(u8),
    Month(u8),
    Annual,
    /// Pago fraccionado del 202 (1.º abril, 2.º octubre, 3.º diciembre).
    Installment(u8),
}

impl Period {
    /// Código del periodo como en los modelos AEAT: `1T`, `01`, `0A`, `1P`.
    pub fn code(self) -> String {
        match self {
            Period::Quarter(q) => format!("{q}T"),
            Period::Month(m) => format!("{m:02}"),
            Period::Annual => "0A".to_owned(),
            Period::Installment(n) => format!("{n}P"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Persona {
    Fisica,
    Juridica,
    /// Comunidad de bienes, sociedad civil sin personalidad… (régimen de atribución).
    EntidadAtribucion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrpfMethod {
    EstimacionDirecta,
    Modulos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Periodicity {
    Mensual,
    Trimestral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VatProfile {
    pub periodicity: Periodicity,
    /// Lleva los libros por SII (mensual hasta el día 30 y exonerado del 390 y del 347). El
    /// periodo mensual (RIVA art. 71.3) obliga al SII (RIVA art. 62.6): ver [`VatProfile::sii`].
    pub sii: bool,
}

impl VatProfile {
    /// Lleva los libros por la Sede (SII): lo marcado o, en todo caso, con periodo mensual,
    /// porque en territorio común no existe el 303 mensual sin SII (RIVA arts. 62.6 y 71.3).
    pub fn sii(self) -> bool {
        self.sii || self.periodicity == Periodicity::Mensual
    }
}

/// Perfil derivado del censo (036/037) que decide qué modelos tocan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxpayerProfile {
    pub persona: Persona,
    /// Persona física con actividad económica.
    pub irpf_method: Option<IrpfMethod>,
    /// Profesional con ≥ 70 % de ingresos sometidos a retención (no presenta el 130;
    /// RIRPF art. 109.2).
    pub exento_pagos_fraccionados: bool,
    /// `None` = sin obligaciones periódicas de IVA (p. ej. actividad exenta).
    pub iva: Option<VatProfile>,
    pub retiene_trabajo_profesionales: bool,
    pub retiene_alquileres: bool,
    pub retiene_capital_mobiliario: bool,
    /// Paga rentas con retención a no residentes sin establecimiento permanente (216/296).
    #[serde(default)]
    pub retiene_no_residentes: bool,
    /// Gran empresa (> 6.010.121,04 €): retenciones mensuales.
    pub gran_empresa: bool,
    pub intracomunitarias: Option<Periodicity>,
    /// Operaciones con un tercero > 3.005,06 €. Con SII no se presenta el 347 (RGAT art.
    /// 32.1.e) aunque esté marcado.
    pub obligado_347: bool,
    /// Sociedades: último día del ejercicio (mes, día).
    pub cierre_ejercicio: (Month, u8),
    /// Sociedades con pagos fraccionados del IS (202).
    pub pagos_fraccionados_is: bool,
    /// Comunidad autónoma del domicilio fiscal: sus festivos también prorrogan los plazos.
    #[serde(default)]
    pub ccaa: Option<Ccaa>,
    /// Festivos locales del municipio del domicilio fiscal.
    #[serde(default)]
    pub festivos_locales: Vec<Date>,
    /// Cuentas, valores o inmuebles en el extranjero por más de 50.000 € (720).
    #[serde(default)]
    pub bienes_extranjero: bool,
    /// Criptomonedas en plataformas del extranjero por más de 50.000 € (721).
    #[serde(default)]
    pub monedas_virtuales_extranjero: bool,
    /// Sociedad con operaciones vinculadas por encima de los umbrales del 232.
    #[serde(default)]
    pub operaciones_vinculadas: bool,
    /// Renta: pago fraccionado en dos plazos (60 % + 40 % en noviembre).
    #[serde(default)]
    pub renta_fraccionada: bool,
    /// Primer año en que se paga el IAE (cifra de negocios de 1 M€ o más dos años antes).
    #[serde(default)]
    pub iae_desde: Option<i32>,
    /// Ventanilla única de la UE: régimen de la Unión o exterior (369 trimestral).
    #[serde(default)]
    pub oss: bool,
    /// Ventanilla única de importación (IOSS, 369 mensual).
    #[serde(default)]
    pub ioss: bool,
    /// Avisar de los plazos para cambiar de régimen en el censo (noviembre y diciembre).
    #[serde(default)]
    pub opciones_censales: bool,
    /// Autónomo dado de alta en el RETA (cuota mensual).
    #[serde(default)]
    pub cotiza_reta: bool,
    /// Con trabajadores: seguros sociales mensuales.
    #[serde(default)]
    pub tiene_trabajadores: bool,
}

impl TaxpayerProfile {
    /// Autónomo en estimación directa, IVA trimestral, sin retenciones.
    pub fn autonomo() -> Self {
        TaxpayerProfile {
            persona: Persona::Fisica,
            irpf_method: Some(IrpfMethod::EstimacionDirecta),
            exento_pagos_fraccionados: false,
            iva: Some(VatProfile {
                periodicity: Periodicity::Trimestral,
                sii: false,
            }),
            retiene_trabajo_profesionales: false,
            retiene_alquileres: false,
            retiene_capital_mobiliario: false,
            retiene_no_residentes: false,
            gran_empresa: false,
            intracomunitarias: None,
            obligado_347: false,
            cierre_ejercicio: (Month::December, 31),
            pagos_fraccionados_is: false,
            ccaa: None,
            festivos_locales: Vec::new(),
            bienes_extranjero: false,
            monedas_virtuales_extranjero: false,
            operaciones_vinculadas: false,
            renta_fraccionada: false,
            iae_desde: None,
            oss: false,
            ioss: false,
            opciones_censales: false,
            cotiza_reta: false,
            tiene_trabajadores: false,
        }
    }

    /// SL de ejercicio natural, IVA trimestral, con nóminas o profesionales y local alquilado.
    pub fn sociedad_limitada() -> Self {
        TaxpayerProfile {
            persona: Persona::Juridica,
            irpf_method: None,
            retiene_trabajo_profesionales: true,
            retiene_alquileres: true,
            pagos_fraccionados_is: true,
            ..TaxpayerProfile::autonomo()
        }
    }
}

/// Un plazo concreto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deadline {
    pub model: Model,
    /// Ejercicio al que corresponde el periodo.
    pub fiscal_year: i32,
    pub period: Period,
    pub starts_on: Date,
    /// Último día según la norma, antes de trasladar por inhábil.
    pub nominal_end: Date,
    /// Último día real (trasladado al siguiente hábil si hace falta).
    pub due_on: Date,
    /// Último día para domiciliar el pago, si aplica.
    pub domiciliation_end: Option<Date>,
    /// `true` si `domiciliation_end` viene de la tabla oficial de la AEAT; `false` si es una
    /// estimación (la AEAT no sigue una regla fija, ver `docs/CALENDAR_RULES.md`).
    pub domiciliation_official: bool,
    /// Fechas estimadas: la norma anual que las fija aún no se ha publicado o verificado.
    pub tentative: bool,
    pub rule_ref: &'static str,
}

fn ymd(year: i32, month: Month, day: u8) -> Date {
    Date::from_calendar_date(year, month, day).expect("fecha válida en las reglas")
}

fn last_day(year: i32, month: Month) -> Date {
    ymd(year, month, month.length(year))
}

/// Suma meses manteniendo el día, recortado al último día del mes (cómputo «de fecha a
/// fecha» del Código Civil art. 5.1: 30/06 + 6 meses = 30/12).
pub fn add_months(d: Date, months: i32) -> Date {
    let total = d.year() * 12 + (d.month() as i32 - 1) + months;
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12));
    let month = Month::try_from(u8::try_from(m0 + 1).unwrap_or(1)).unwrap_or(Month::January);
    ymd(y, month, d.day().min(month.length(y)))
}

/// Como `add_months`, pero un último día de mes lleva al último día del mes destino
/// (30/06 + 6 meses = 31/12): para los plazos que acaban con un mes entero («los seis primeros
/// meses del ejercicio», LSC art. 164), no para los que se cuentan de fecha a fecha.
pub fn add_months_eom(d: Date, months: i32) -> Date {
    let r = add_months(d, months);
    if d.day() == d.month().length(d.year()) {
        last_day(r.year(), r.month())
    } else {
        r
    }
}

fn month_after_quarter(q: u8) -> Month {
    Month::try_from(q * 3 + 1).unwrap_or(Month::January)
}

/// Qué hacer si el último día del plazo es inhábil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shift {
    /// Al primer día hábil siguiente (Ley 39/2015 art. 30.5): la regla general.
    Forward,
    /// No se mueve (modelo 369: Reglamento de Ejecución (UE) 282/2011 y calendario AEAT).
    Fixed,
    /// Al último día hábil anterior: cargos de la Seguridad Social.
    Backward,
}

struct Raw {
    model: Model,
    fiscal_year: i32,
    period: Period,
    start: Date,
    end: Date,
    tentative: bool,
    rule_ref: &'static str,
    shift: Shift,
}

/// Plazos trimestrales: días 1–20 del mes siguiente; el 4T hasta el `q4_end` de enero.
fn quarterly(model: Model, fy: i32, q4_end: u8, rule_ref: &'static str) -> Vec<Raw> {
    (1..=4)
        .map(|q| {
            let (start, end) = if q < 4 {
                let m = month_after_quarter(q);
                (ymd(fy, m, 1), ymd(fy, m, 20))
            } else {
                (
                    ymd(fy + 1, Month::January, 1),
                    ymd(fy + 1, Month::January, q4_end),
                )
            };
            Raw {
                model,
                fiscal_year: fy,
                period: Period::Quarter(q),
                start,
                end,
                tentative: false,
                rule_ref,
                shift: Shift::Forward,
            }
        })
        .collect()
}

/// Plazos mensuales: días 1–`day` del mes siguiente; diciembre hasta el `dec_end` de enero.
fn monthly(model: Model, fy: i32, day: u8, dec_end: u8, rule_ref: &'static str) -> Vec<Raw> {
    (1..=12u8)
        .map(|m| {
            let (start, end) = if m < 12 {
                let next = Month::try_from(m + 1).unwrap_or(Month::January);
                (ymd(fy, next, 1), ymd(fy, next, day.min(next.length(fy))))
            } else {
                (
                    ymd(fy + 1, Month::January, 1),
                    ymd(fy + 1, Month::January, dec_end),
                )
            };
            Raw {
                model,
                fiscal_year: fy,
                period: Period::Month(m),
                start,
                end,
                tentative: false,
                rule_ref,
                shift: Shift::Forward,
            }
        })
        .collect()
}

fn annual(model: Model, fy: i32, start: Date, end: Date, rule_ref: &'static str) -> Raw {
    Raw {
        model,
        fiscal_year: fy,
        period: Period::Annual,
        start,
        end,
        tentative: false,
        rule_ref,
        shift: Shift::Forward,
    }
}

/// Obligaciones del ejercicio `fy` (algunas vencen en `fy + 1`).
fn obligations_for_fiscal_year(p: &TaxpayerProfile, fy: i32) -> Vec<Raw> {
    let mut out = Vec::new();
    let fisica = p.persona == Persona::Fisica;
    let sociedad = p.persona == Persona::Juridica;

    if let Some(iva) = p.iva {
        let r = "RIVA art. 71.4 y 71.5 (RD 1624/1992); Orden del modelo 303";
        match iva.periodicity {
            Periodicity::Trimestral => out.extend(quarterly(Model::M303, fy, 30, r)),
            // «Treinta primeros días naturales del mes siguiente» (febrero: hasta su último
            // día); diciembre, hasta el 30 de enero. RIVA art. 71.4, párrafos 2.º y 3.º.
            Periodicity::Mensual => out.extend(monthly(Model::M303, fy, 30, 30, r)),
        }
        if !iva.sii() {
            // TODO(VERIFY): exoneraciones del 390 distintas del SII.
            out.push(annual(
                Model::M390,
                fy,
                ymd(fy + 1, Month::January, 1),
                ymd(fy + 1, Month::January, 30),
                "RIVA art. 71.7; Orden del modelo 390",
            ));
        }
    }

    match (fisica, p.irpf_method) {
        (true, Some(IrpfMethod::EstimacionDirecta)) if !p.exento_pagos_fraccionados => {
            out.extend(quarterly(
                Model::M130,
                fy,
                30,
                "RIRPF art. 111.1 (RD 439/2007)",
            ));
        }
        (true, Some(IrpfMethod::Modulos)) => {
            out.extend(quarterly(
                Model::M131,
                fy,
                30,
                "RIRPF art. 111.1 (RD 439/2007)",
            ));
        }
        _ => {}
    }

    // Retenciones: 1–20 del mes siguiente; el 4T y diciembre, 1–20 de enero.
    for (on, model, summary, r) in [
        (
            p.retiene_trabajo_profesionales,
            Model::M111,
            Model::M190,
            "RIRPF art. 108.1; RIS art. 68 (RD 634/2015)",
        ),
        (
            p.retiene_alquileres,
            Model::M115,
            Model::M180,
            "RIRPF art. 108.1; RIS art. 68 (RD 634/2015)",
        ),
        (
            p.retiene_capital_mobiliario,
            Model::M123,
            Model::M193,
            "RIRPF art. 108.1; RIS art. 68 (RD 634/2015)",
        ),
        // TODO(VERIFY): plazos del 216 (mensual para las grandes empresas, como el 111) y del
        // 296 (enero) en la Orden EHA/3290/2008.
        (
            p.retiene_no_residentes,
            Model::M216,
            Model::M296,
            "TRLIRNR art. 31; Orden EHA/3290/2008",
        ),
    ] {
        if !on {
            continue;
        }
        if p.gran_empresa {
            out.extend(monthly(model, fy, 20, 20, r));
        } else {
            out.extend(quarterly(model, fy, 20, r));
        }
        out.push(annual(
            summary,
            fy,
            ymd(fy + 1, Month::January, 1),
            ymd(fy + 1, Month::January, 31),
            "RIRPF art. 108.3; Orden del modelo resumen",
        ));
    }

    if let Some(per) = p.intracomunitarias {
        let r = "RIVA art. 81; Orden EHA/769/2010 (modelo 349)";
        match per {
            Periodicity::Mensual => {
                let mut ms = monthly(Model::M349, fy, 20, 30, r);
                // Julio se presenta con agosto: hasta el 20 de septiembre (RIVA art. 81.2).
                if let Some(july) = ms.iter_mut().find(|x| x.period == Period::Month(7)) {
                    july.end = ymd(fy, Month::September, 20);
                }
                out.extend(ms);
            }
            Periodicity::Trimestral => out.extend(quarterly(Model::M349, fy, 30, r)),
        }
    }

    // Quien lleva los libros por el SII (también REDEME y mensuales) está excluido del 347 del
    // todo: RGAT art. 32.1.e (añadido por el RD 596/2016) y, para el ejercicio 2017 en adelante,
    // la Orden HFP/1106/2017 (BOE 18-11-2017), que suprimió el 347 «reducido» (subvenciones,
    // arrendamientos de locales, cobros en metálico del art. 34.1 RGAT) que el RD 596/2016 había
    // dejado para los del SII. Con SII no se genera ningún 347.
    if p.obligado_347 && !p.iva.is_some_and(VatProfile::sii) {
        out.push(annual(
            Model::M347,
            fy,
            ymd(fy + 1, Month::February, 1),
            last_day(fy + 1, Month::February),
            "Orden EHA/3012/2008 art. 10; RGAT arts. 31-33 (RD 1065/2007)",
        ));
    }

    if p.persona == Persona::EntidadAtribucion {
        out.push(annual(
            Model::M184,
            fy,
            ymd(fy + 1, Month::January, 1),
            ymd(fy + 1, Month::January, 31),
            "RIRPF art. 70; Orden del modelo 184",
        ));
    }

    if fisica {
        let renta_year = fy + 1;
        let known = official::find("100", fy, "0A").and_then(|e| e.start);
        let start = known.unwrap_or_else(|| ymd(renta_year, Month::April, 1));
        out.push(Raw {
            model: Model::M100,
            fiscal_year: fy,
            period: Period::Annual,
            start,
            end: ymd(renta_year, Month::June, 30),
            tentative: known.is_none(),
            rule_ref: "Orden anual del modelo 100 (campaña de Renta)",
            shift: Shift::Forward,
        });
    }

    if sociedad {
        let (m, d) = p.cierre_ejercicio;
        let close_year = if (m, d) == (Month::December, 31) {
            fy
        } else {
            fy + 1
        };
        let close = ymd(close_year, m, d.min(m.length(close_year)));
        // Los seis meses del art. 124.1 LIS se cuentan de fecha a fecha (Código Civil art.
        // 5.1): un cierre el 30/06 acaba el 30/12 y los 25 días van del 31/12 al 24/01 (AEAT,
        // folleto de actividades económicas 4.9.1). Con cierre el 31/12 sale 1–25 de julio.
        let six = add_months(close, 6);
        out.push(annual(
            Model::M200,
            fy,
            six + Duration::days(1),
            six + Duration::days(25),
            "LIS art. 124.1 (Ley 27/2014): 25 días naturales tras los 6 meses del cierre",
        ));
        if p.pagos_fraccionados_is && p.opciones_censales {
            // Opción (o renuncia) por la modalidad de la base del 202 (art. 40.3 LIS, párrafo
            // cuarto): «durante el mes de febrero» si el período coincide con el año natural; si
            // no, en los dos primeros meses del período o hasta el fin del plazo del primer pago
            // si es menor. TODO(VERIFY): plazo y traslado por inhábil.
            let start = close_of(p, fy - 1) + Duration::days(1);
            let window = if (m, d) == (Month::December, 31) {
                (ymd(fy, Month::February, 1), last_day(fy, Month::February))
            } else {
                let two = add_months(start, 2) - Duration::days(1);
                let first_payment = (start.year()..=start.year() + 1)
                    .flat_map(|y| [Month::April, Month::October, Month::December].map(|m| (y, m)))
                    .map(|(y, m)| ymd(y, m, 20))
                    .find(|x| *x >= start);
                (start, first_payment.map_or(two, |f| two.min(f)))
            };
            out.push(raw(
                Model::Opcion202Base,
                fy,
                Period::Annual,
                window,
                Shift::Forward,
                "LIS art. 40.3 (Ley 27/2014): opción por la base en el 036, en febrero o en los \
                 dos primeros meses del período",
            ));
        }
        if p.pagos_fraccionados_is {
            for (n, month) in [(1, Month::April), (2, Month::October), (3, Month::December)] {
                out.push(Raw {
                    model: Model::M202,
                    fiscal_year: fy,
                    period: Period::Installment(n),
                    start: ymd(fy, month, 1),
                    end: ymd(fy, month, 20),
                    tentative: false,
                    rule_ref: "LIS art. 40.1: primeros 20 días naturales de abril, octubre y diciembre",
                    shift: Shift::Forward,
                });
            }
        }
        // Registro Mercantil. TODO(VERIFY): traslado por inhábiles en el Registro. Los cuatro
        // meses, de fecha a fecha (CC art. 5.1): cierre el 30/06 → 30/10.
        out.push(annual(
            Model::LegalizacionLibros,
            fy,
            close + Duration::days(1),
            add_months(close, 4),
            "Ley 14/2013 art. 18: 4 meses desde el cierre",
        ));
        // La junta se celebra «dentro de los seis primeros meses de cada ejercicio» (LSC art.
        // 164), es decir, hasta el último día del sexto mes (31/12 con cierre el 30/06): aquí
        // sí vale el cómputo a fin de mes.
        out.push(annual(
            Model::DepositoCuentas,
            fy,
            close + Duration::days(1),
            add_months(add_months_eom(close, 6), 1),
            "LSC arts. 164 y 279: aprobación en 6 meses + 1 mes para depositar",
        ));
    }
    out.extend(other_obligations(p, fy));
    out
}

/// Último día del ejercicio `fy` de una sociedad (el del año siguiente si no es natural).
fn close_of(p: &TaxpayerProfile, fy: i32) -> Date {
    let (m, d) = p.cierre_ejercicio;
    let year = if (m, d) == (Month::December, 31) {
        fy
    } else {
        fy + 1
    };
    ymd(year, m, d.min(m.length(year)))
}

fn raw(
    model: Model,
    fiscal_year: i32,
    period: Period,
    (start, end): (Date, Date),
    shift: Shift,
    rule_ref: &'static str,
) -> Raw {
    Raw {
        model,
        fiscal_year,
        period,
        start,
        end,
        tentative: false,
        rule_ref,
        shift,
    }
}

/// Informativas, censo, IAE, ventanilla única y Seguridad Social del ejercicio `fy`: sólo las
/// que el perfil marca. Ver `docs/CALENDAR_RULES.md` §6.
fn other_obligations(p: &TaxpayerProfile, fy: i32) -> Vec<Raw> {
    let mut out = Vec::new();
    let fisica = p.persona == Persona::Fisica;
    let next = fy + 1;
    let jan_mar = (ymd(next, Month::January, 1), ymd(next, Month::March, 31));

    // 720 y 721: «entre el 1 de enero y el 31 de marzo del año siguiente».
    if p.bienes_extranjero {
        out.push(raw(
            Model::M720,
            fy,
            Period::Annual,
            jan_mar,
            Shift::Forward,
            "Orden HAP/72/2013 art. 7; RGAT arts. 42 bis, 42 ter y 54 bis (RD 1065/2007)",
        ));
    }
    // El 721 se presentó por primera vez en 2024 (ejercicio 2023).
    if p.monedas_virtuales_extranjero && fy >= 2023 {
        // Orden HFP/886/2023 art. 4 (BOE-A-2023-17429, leído el 2026-10-27): «entre el 1 de
        // enero y el 31 de marzo del año siguiente a aquel al que se refiera la información».
        out.push(raw(
            Model::M721,
            fy,
            Period::Annual,
            jan_mar,
            Shift::Forward,
            "Orden HFP/886/2023 art. 4; RGAT art. 42 quater (RD 1065/2007)",
        ));
    }

    // 232: «en el mes siguiente a los diez meses posteriores a la conclusión del período
    // impositivo» (ejercicio natural: noviembre del año siguiente).
    if p.persona == Persona::Juridica && p.operaciones_vinculadas {
        // TODO(VERIFY): en ejercicios no naturales se cuentan meses enteros (cierre el 30/06 →
        // mayo); falta confirmarlo con un ejemplo de la AEAT.
        let ten = add_months_eom(close_of(p, fy), 10);
        out.push(raw(
            Model::M232,
            fy,
            Period::Annual,
            (ten + Duration::days(1), add_months_eom(ten, 1)),
            Shift::Forward,
            "Orden HFP/816/2017 art. 4; RIS art. 13.4 (RD 634/2015)",
        ));
    }

    // Renta: el 40 % restante, hasta el 5 de noviembre (lo fija la orden anual; Renta 2025:
    // Orden HAC/277/2026). Con la campaña sin orden publicada, la fecha es estimada.
    if fisica && p.renta_fraccionada {
        out.push(Raw {
            tentative: official::find("100", fy, "0A").is_none(),
            ..raw(
                Model::M100SegundoPlazo,
                fy,
                Period::Installment(2),
                (ymd(next, Month::July, 1), ymd(next, Month::November, 5)),
                Shift::Forward,
                "RIRPF art. 62.2 (RD 439/2007); Orden anual del modelo 100",
            )
        });
    }

    // IAE: las personas físicas están exentas siempre; las demás, mientras su cifra de
    // negocios del penúltimo año sea menor de 1 M€ (TRLRHL art. 82.1.c).
    if let Some(desde) = p.iae_desde.filter(|_| !fisica) {
        // Alta de quien deja de estar exento: art. 5.3, párrafo segundo, del RD 243/1995
        // (BOE-A-1995-5917, leído el 2026-10-27): «se presentarán durante el mes de diciembre
        // inmediato anterior al año en el que el sujeto pasivo resulte obligado a contribuir».
        if next == desde {
            out.push(raw(
                Model::M840,
                desde,
                Period::Annual,
                (ymd(fy, Month::December, 1), ymd(fy, Month::December, 31)),
                Shift::Forward,
                "RD 243/1995 art. 5.3; TRLRHL art. 82.1.c (RDLeg 2/2004)",
            ));
        }
        // 848: cifra de negocios del penúltimo año, «entre el día 1 de enero y el día 14 de
        // febrero, ambos incluidos» (Orden HAC/85/2003 apartado tercero, BOE-A-2003-1686, leído
        // el 2026-10-27). Están exonerados (apartado primero) los exentos del IAE y quienes ya
        // hicieron constar la cifra en el 200 (sociedades y no residentes con EP) o en el 184
        // (sociedades civiles y entidades del art. 35.4 LGT), así que sólo queda para la entidad
        // en atribución que no la declaró en el 184; la app lo avisa con esa salvedad.
        if p.persona == Persona::EntidadAtribucion && fy + 2 >= desde {
            out.push(raw(
                Model::M848,
                fy,
                Period::Annual,
                (
                    ymd(fy + 2, Month::January, 1),
                    ymd(fy + 2, Month::February, 14),
                ),
                Shift::Forward,
                "Orden HAC/85/2003 apartados primero y tercero; TRLRHL art. 82.1.c (RDLeg \
                 2/2004)",
            ));
        }
    }

    // 369: el mes siguiente al periodo, sin traslado si acaba en inhábil (calendario AEAT 2026,
    // «Vencimientos en días inhábiles»: «excepto para el modelo 369»).
    let r369 = "LIVA arts. 163 unvicies y ss. (Ley 37/1992); Directiva 2006/112/CE arts. 364 y \
                369 septies; Orden HAC/610/2021";
    let month_after = |y: i32, m: u8| {
        let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        let month = Month::try_from(m).unwrap_or(Month::January);
        (ymd(y, month, 1), last_day(y, month))
    };
    if p.oss {
        for q in 1..=4u8 {
            out.push(raw(
                Model::M369,
                fy,
                Period::Quarter(q),
                month_after(fy, q * 3),
                Shift::Fixed,
                r369,
            ));
        }
    }
    if p.ioss {
        for m in 1..=12u8 {
            out.push(raw(
                Model::M369,
                fy,
                Period::Month(m),
                month_after(fy, m),
                Shift::Fixed,
                r369,
            ));
        }
    }

    // Opciones censales del año siguiente (efectos en `next`).
    if p.opciones_censales {
        if p.iva.is_some() {
            // SII voluntario (art. 62.6, párrafo segundo, RIVA): la opción «podrá ejercitarse a
            // lo largo de todo el ejercicio» con efectos en el primer periodo de liquidación
            // siguiente, y la renuncia «se deberá formular en el mes de noviembre anterior al
            // inicio del año natural en el que deba surtir efecto» (art. 68 bis RIVA, redacción
            // del RD 1512/2018; leído en el BOE el 2026-10-27). Aquí sólo cabe la renuncia; la
            // opción no tiene plazo. REDEME: art. 30.4 RIVA.
            out.push(raw(
                Model::OpcionesNoviembre,
                next,
                Period::Annual,
                (ymd(fy, Month::November, 1), ymd(fy, Month::November, 30)),
                Shift::Forward,
                "RIVA art. 30.4 (REDEME) y art. 68 bis (renuncia al SII); RGAT arts. 9 y 10 \
                 (036/037)",
            ));
            // La prorrata especial se elige en el 303 del último periodo del año al que se
            // aplica: art. 28.1.1.º.a) RIVA (redacción del RD 828/2013, leída en el BOE el
            // 2026-10-27): «en la última declaración-liquidación del Impuesto correspondiente a
            // cada año natural, procediéndose en tal caso, a la regularización de las
            // deducciones practicadas durante el mismo»; con inicio de actividad, letra b).
            // Vale un mínimo de tres años y se revoca igual, en la última del año.
            out.push(raw(
                Model::ProrrataEspecial,
                fy,
                Period::Annual,
                (ymd(next, Month::January, 1), ymd(next, Month::January, 30)),
                Shift::Forward,
                "RIVA art. 28.1.1.º.a (RD 1624/1992)",
            ));
        }
        let irpf = fisica && p.irpf_method.is_some();
        if irpf || p.iva.is_some_and(|i| !i.sii()) {
            out.push(raw(
                Model::OpcionesDiciembre,
                next,
                Period::Annual,
                (ymd(fy, Month::December, 1), ymd(fy, Month::December, 31)),
                Shift::Forward,
                "RIRPF arts. 28.1 y 33.1.a (RD 439/2007); RIVA arts. 33.2 y 61 septies",
            ));
        }
    }

    // Seguridad Social: la cuota del RETA se ingresa dentro del mismo mes (RGR-SS art. 56.1.b
    // 1.º, RD 1415/2004, leído en el BOE el 2026-10-27) y la Guía práctica de trabajo autónomo
    // de la TGSS (portal.seg-social.gob.es, Import@ss) dice: «La cuota se cargará en tu cuenta
    // bancaria el último día hábil de cada mes». Por eso se adelanta en vez de retrasarse.
    // En una sociedad, `cotiza_reta` es el administrador o socio con control que cotiza como
    // autónomo societario (art. 305.2.b y e TRLGSS): su cuota y sus cambios de base.
    if p.cotiza_reta {
        for m in 1..=12u8 {
            let month = Month::try_from(m).unwrap_or(Month::January);
            out.push(raw(
                Model::CuotaAutonomos,
                fy,
                Period::Month(m),
                (ymd(fy, month, 1), last_day(fy, month)),
                Shift::Backward,
                "RD 1415/2004 art. 56 (Reglamento General de Recaudación de la Seguridad \
                 Social): la cuota del RETA se ingresa dentro del mismo mes",
            ));
        }
        // Cambios de base: seis al año, pedidos dentro de cada bimestre y con efectos el día 1
        // del mes siguiente (art. 308.1.a, regla 3.ª, TRLGSS; art. 45.1 del RD 2064/1995 en la
        // redacción del RDL 13/2022, BOE-A-1996-1579: el art. 43 bis está derogado desde el
        // 1-1-2023). La solicitud es telemática (Import@ss) y el plazo no se mueve si acaba en
        // festivo.
        for v in crate::reta::ventanas_cambio(fy) {
            out.push(raw(
                Model::CambioBaseReta,
                fy,
                Period::Month(u8::from(v.hasta.month())),
                (v.desde, v.hasta),
                Shift::Fixed,
                "Art. 308.1.a TRLGSS y art. 45.1 RD 2064/1995 (RDL 13/2022): hasta seis \
                 cambios de base al año, con efectos el día 1 del mes siguiente al bimestre",
            ));
        }
    }
    if p.tiene_trabajadores {
        for m in 1..=12u8 {
            out.push(raw(
                Model::SegurosSociales,
                fy,
                Period::Month(m),
                month_after(fy, m),
                Shift::Backward,
                "RD 1415/2004 art. 56 (Reglamento General de Recaudación de la Seguridad \
                 Social): mes natural siguiente al del devengo",
            ));
        }
    }
    out
}

/// Plazos que **vencen** en el año natural `year`, ordenados por fecha.
pub fn deadlines_in_year(
    p: &TaxpayerProfile,
    year: i32,
    holidays: &HolidayCalendar,
) -> Vec<Deadline> {
    // Un ejercicio no natural puede vencer hasta dos años después de empezar (200).
    let mut out: Vec<Deadline> = (year - 2..=year)
        .flat_map(|fy| obligations_for_fiscal_year(p, fy))
        .map(|r| {
            let due_on = match r.shift {
                Shift::Forward => holidays.roll_forward(r.end),
                Shift::Fixed => r.end,
                Shift::Backward => holidays.roll_back(r.end),
            };
            let official = official::find(r.model.code(), r.fiscal_year, &r.period.code())
                .and_then(|e| e.end_domiciliacion);
            let domiciliation_end = r.model.is_self_assessment().then(|| {
                // Estimación: 5 días naturales antes del último día real, trasladado a hábil.
                // TODO(VERIFY): la AEAT publica cada año su tabla y a veces difiere.
                official.unwrap_or_else(|| holidays.roll_forward(due_on - Duration::days(5)))
            });
            Deadline {
                model: r.model,
                fiscal_year: r.fiscal_year,
                period: r.period,
                starts_on: r.start,
                nominal_end: r.end,
                due_on,
                domiciliation_end,
                domiciliation_official: official.is_some(),
                tentative: r.tentative,
                rule_ref: r.rule_ref,
            }
        })
        .filter(|d| d.due_on.year() == year)
        .collect();
    out.sort_by_key(|d| (d.due_on, d.model, d.period));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn find(ds: &[Deadline], m: Model, fy: i32, p: Period) -> Deadline {
        ds.iter()
            .find(|d| d.model == m && d.fiscal_year == fy && d.period == p)
            .unwrap_or_else(|| panic!("falta {} {fy} {}", m.code(), p.code()))
            .clone()
    }

    #[test]
    fn autonomo_trimestral_2026() {
        let ds = deadlines_in_year(
            &TaxpayerProfile::autonomo(),
            2026,
            &HolidayCalendar::default(),
        );
        let d = find(&ds, Model::M303, 2026, Period::Quarter(1));
        assert_eq!(
            (d.starts_on, d.nominal_end),
            (date!(2026 - 04 - 01), date!(2026 - 04 - 20))
        );
        assert_eq!(d.domiciliation_end, Some(date!(2026 - 04 - 15)));
        let d = find(&ds, Model::M303, 2025, Period::Quarter(4));
        assert_eq!(d.nominal_end, date!(2026 - 01 - 30));
        let d = find(&ds, Model::M130, 2025, Period::Quarter(4));
        assert_eq!(d.nominal_end, date!(2026 - 01 - 30));
        assert!(
            ds.iter()
                .any(|d| d.model == Model::M390 && d.fiscal_year == 2025)
        );
        assert!(
            ds.iter()
                .any(|d| d.model == Model::M100 && d.fiscal_year == 2025)
        );
        // El 4T de 2026 vence en 2027: no aparece.
        assert!(
            !ds.iter()
                .any(|d| d.fiscal_year == 2026 && d.period == Period::Quarter(4))
        );
        assert!(
            !ds.iter()
                .any(|d| matches!(d.model, Model::M111 | Model::M200))
        );
        assert!(ds.windows(2).all(|w| w[0].due_on <= w[1].due_on));
    }

    #[test]
    fn sociedad_limitada_2026() {
        let ds = deadlines_in_year(
            &TaxpayerProfile::sociedad_limitada(),
            2026,
            &HolidayCalendar::default(),
        );
        let d = find(&ds, Model::M111, 2025, Period::Quarter(4));
        assert_eq!(d.nominal_end, date!(2026 - 01 - 20));
        let d = find(&ds, Model::M200, 2025, Period::Annual);
        assert_eq!(
            (d.starts_on, d.nominal_end),
            (date!(2026 - 07 - 01), date!(2026 - 07 - 25))
        );
        let d = find(&ds, Model::M202, 2026, Period::Installment(3));
        assert_eq!(d.nominal_end, date!(2026 - 12 - 20));
        let d = find(&ds, Model::DepositoCuentas, 2025, Period::Annual);
        assert_eq!(d.nominal_end, date!(2026 - 07 - 30));
        let d = find(&ds, Model::LegalizacionLibros, 2025, Period::Annual);
        assert_eq!(d.nominal_end, date!(2026 - 04 - 30));
        assert!(
            ds.iter().any(|d| d.model == Model::M190) && ds.iter().any(|d| d.model == Model::M180)
        );
        assert!(
            !ds.iter()
                .any(|d| matches!(d.model, Model::M130 | Model::M100))
        );
    }

    #[test]
    fn ejercicio_no_natural() {
        let p = TaxpayerProfile {
            cierre_ejercicio: (Month::June, 30),
            ..TaxpayerProfile::sociedad_limitada()
        };
        // Ejercicio 2025 = 1/7/2025–30/6/2026: los seis meses acaban el 30/12/2026 y el 200 se
        // presenta del 31/12/2026 al 24/01/2027 (AEAT, folleto actividades económicas 4.9.1).
        let ds = deadlines_in_year(&p, 2027, &HolidayCalendar::default());
        let d = find(&ds, Model::M200, 2025, Period::Annual);
        assert_eq!(
            (d.starts_on, d.nominal_end),
            (date!(2026 - 12 - 31), date!(2027 - 01 - 24))
        );
        // El depósito sigue la junta de los «seis primeros meses» (hasta el 31/12) + 1 mes.
        let d = find(&ds, Model::DepositoCuentas, 2025, Period::Annual);
        assert_eq!(d.nominal_end, date!(2027 - 01 - 31));
    }

    #[test]
    fn ejercicio_no_natural_cierres_en_dia_30() {
        // Cierre 31/03/2026: 6 meses → 30/09; 200 del 01/10 al 25/10/2026.
        let p = TaxpayerProfile {
            cierre_ejercicio: (Month::March, 31),
            ..TaxpayerProfile::sociedad_limitada()
        };
        let ds = deadlines_in_year(&p, 2026, &HolidayCalendar::default());
        let d = find(&ds, Model::M200, 2025, Period::Annual);
        assert_eq!(
            (d.starts_on, d.nominal_end),
            (date!(2026 - 10 - 01), date!(2026 - 10 - 25))
        );
        // Cierre 30/09/2025: 6 meses → 30/03/2026; 200 del 31/03 al 24/04/2026.
        let p = TaxpayerProfile {
            cierre_ejercicio: (Month::September, 30),
            ..TaxpayerProfile::sociedad_limitada()
        };
        let ds = deadlines_in_year(&p, 2026, &HolidayCalendar::default());
        let d = find(&ds, Model::M200, 2024, Period::Annual);
        assert_eq!(
            (d.starts_on, d.nominal_end),
            (date!(2026 - 03 - 31), date!(2026 - 04 - 24))
        );
        // Legalización: 4 meses de fecha a fecha desde el 30/09/2025 → 30/01/2026.
        let d = find(&ds, Model::LegalizacionLibros, 2024, Period::Annual);
        assert_eq!(d.nominal_end, date!(2026 - 01 - 30));
    }

    #[test]
    fn mensual_implica_sii_y_el_sii_excluye_390_y_347() {
        let cal = HolidayCalendar::default();
        // Mensual sin marcar SII no existe en territorio común (RIVA art. 62.6): ni 390 ni 347.
        let p = TaxpayerProfile {
            iva: Some(VatProfile {
                periodicity: Periodicity::Mensual,
                sii: false,
            }),
            obligado_347: true,
            ..TaxpayerProfile::sociedad_limitada()
        };
        let ds = deadlines_in_year(&p, 2026, &cal);
        assert!(
            ds.iter()
                .any(|d| d.model == Model::M303 && d.period == Period::Month(1))
        );
        assert!(
            !ds.iter()
                .any(|d| matches!(d.model, Model::M390 | Model::M347))
        );
        // Trimestral con SII marcado: tampoco presenta el 347 (RGAT art. 32.1.e).
        let p = TaxpayerProfile {
            iva: Some(VatProfile {
                periodicity: Periodicity::Trimestral,
                sii: true,
            }),
            obligado_347: true,
            ..TaxpayerProfile::autonomo()
        };
        let ds = deadlines_in_year(&p, 2026, &cal);
        assert!(
            !ds.iter()
                .any(|d| matches!(d.model, Model::M390 | Model::M347))
        );
        // Sin SII, el 347 sigue.
        let p = TaxpayerProfile {
            obligado_347: true,
            ..TaxpayerProfile::autonomo()
        };
        let ds = deadlines_in_year(&p, 2026, &cal);
        assert!(ds.iter().any(|d| d.model == Model::M347));
    }

    #[test]
    fn cada_regla_cita_su_norma() {
        // Perfiles que, juntos, generan todos los modelos del motor.
        let perfiles = [
            TaxpayerProfile {
                retiene_trabajo_profesionales: true,
                retiene_alquileres: true,
                retiene_capital_mobiliario: true,
                retiene_no_residentes: true,
                intracomunitarias: Some(Periodicity::Mensual),
                obligado_347: true,
                ..TaxpayerProfile::autonomo()
            },
            TaxpayerProfile {
                irpf_method: Some(IrpfMethod::Modulos),
                ..TaxpayerProfile::autonomo()
            },
            TaxpayerProfile {
                persona: Persona::EntidadAtribucion,
                ..TaxpayerProfile::autonomo()
            },
            TaxpayerProfile::sociedad_limitada(),
            todo_marcado_autonomo(),
            todo_marcado_sociedad(),
        ];
        let ds: Vec<Deadline> = perfiles
            .iter()
            .flat_map(|p| deadlines_in_year(p, 2026, &HolidayCalendar::default()))
            .collect();
        for d in &ds {
            // Un artículo o la orden del modelo; nunca el RGAT art. 35 (imputación temporal),
            // el RIRPF art. 109 (obligados) ni el RIS art. 69 (compensación) para un plazo.
            assert!(
                d.rule_ref.contains("art") || d.rule_ref.contains("Orden"),
                "{} sin norma: {}",
                d.model.code(),
                d.rule_ref
            );
            assert!(!d.rule_ref.contains("art. 35"), "{}", d.rule_ref);
            assert!(!d.rule_ref.contains("art. 109"), "{}", d.rule_ref);
            assert!(!d.rule_ref.contains("RIS art. 69"), "{}", d.rule_ref);
        }
        let m130 = ds.iter().find(|d| d.model == Model::M130).unwrap();
        assert!(m130.rule_ref.contains("art. 111.1"));
        let m347 = ds.iter().find(|d| d.model == Model::M347).unwrap();
        assert!(m347.rule_ref.contains("Orden EHA/3012/2008 art. 10"));
    }

    fn todo_marcado_autonomo() -> TaxpayerProfile {
        TaxpayerProfile {
            bienes_extranjero: true,
            monedas_virtuales_extranjero: true,
            renta_fraccionada: true,
            oss: true,
            ioss: true,
            opciones_censales: true,
            cotiza_reta: true,
            tiene_trabajadores: true,
            // Una persona física nunca paga el IAE: se ignora.
            iae_desde: Some(2020),
            ..TaxpayerProfile::autonomo()
        }
    }

    fn todo_marcado_sociedad() -> TaxpayerProfile {
        TaxpayerProfile {
            operaciones_vinculadas: true,
            iae_desde: Some(2027),
            tiene_trabajadores: true,
            // En una sociedad es el administrador autónomo societario.
            cotiza_reta: true,
            ..TaxpayerProfile::sociedad_limitada()
        }
    }

    const NUEVOS: [Model; 14] = [
        Model::M720,
        Model::M721,
        Model::M232,
        Model::M100SegundoPlazo,
        Model::M848,
        Model::M840,
        Model::M369,
        Model::OpcionesNoviembre,
        Model::OpcionesDiciembre,
        Model::Opcion202Base,
        Model::ProrrataEspecial,
        Model::CuotaAutonomos,
        Model::CambioBaseReta,
        Model::SegurosSociales,
    ];

    #[test]
    fn opcion_del_202_por_la_base_en_febrero() {
        let p = TaxpayerProfile {
            opciones_censales: true,
            ..TaxpayerProfile::sociedad_limitada()
        };
        let (cal, _) = super::super::holidays::calendar_for(2026, None, &[]);
        let ds = deadlines_in_year(&p, 2026, &cal);
        let d = find(&ds, Model::Opcion202Base, 2026, Period::Annual);
        // El 28/02/2026 es sábado: el plazo pasa al lunes 2 de marzo.
        assert_eq!(
            (d.starts_on, d.nominal_end, d.due_on),
            (
                date!(2026 - 02 - 01),
                date!(2026 - 02 - 28),
                date!(2026 - 03 - 02)
            )
        );
        assert_eq!(d.domiciliation_end, None);
        assert_eq!(d.model.title(), "Modelo 036");
        assert!(d.rule_ref.contains("40.3"));
        // Ejercicio de julio a junio: los dos primeros meses (julio y agosto), que acaban antes
        // que el primer pago (20 de octubre).
        let p = TaxpayerProfile {
            cierre_ejercicio: (Month::June, 30),
            ..p
        };
        let ds = deadlines_in_year(&p, 2026, &cal);
        let d = find(&ds, Model::Opcion202Base, 2026, Period::Annual);
        assert_eq!(
            (d.starts_on, d.nominal_end),
            (date!(2026 - 07 - 01), date!(2026 - 08 - 31))
        );
    }

    #[test]
    fn sin_marcar_no_aparecen_las_obligaciones_nuevas() {
        for p in [
            TaxpayerProfile::autonomo(),
            TaxpayerProfile::sociedad_limitada(),
        ] {
            for y in [2026, 2027] {
                let ds = deadlines_in_year(&p, y, &HolidayCalendar::default());
                assert!(!ds.iter().any(|d| NUEVOS.contains(&d.model)), "{y}");
            }
        }
    }

    #[test]
    fn autonomo_720_721_renta_369_censo_y_reta_2026() {
        let (cal, loaded) = super::super::holidays::calendar_for(2026, None, &[]);
        assert!(loaded);
        let ds = deadlines_in_year(&todo_marcado_autonomo(), 2026, &cal);
        for m in [Model::M720, Model::M721] {
            let d = find(&ds, m, 2025, Period::Annual);
            assert_eq!(
                (d.starts_on, d.due_on),
                (date!(2026 - 01 - 01), date!(2026 - 03 - 31))
            );
            assert_eq!(d.domiciliation_end, None);
        }
        // Renta 2025: segundo plazo hasta el jueves 5/11/2026 (Orden HAC/277/2026).
        let d = find(&ds, Model::M100SegundoPlazo, 2025, Period::Installment(2));
        assert_eq!(d.due_on, date!(2026 - 11 - 05));
        assert!(!d.tentative);
        assert_eq!(d.domiciliation_end, None);
        assert_eq!(d.model.title(), "Modelo 100");
        // 369 del 4T 2025: vence el sábado 31/01/2026 y no se traslada.
        let d = find(&ds, Model::M369, 2025, Period::Quarter(4));
        assert_eq!(
            (d.nominal_end, d.due_on),
            (date!(2026 - 01 - 31), date!(2026 - 01 - 31))
        );
        let d = find(&ds, Model::M369, 2026, Period::Quarter(2));
        assert_eq!(
            (d.starts_on, d.due_on),
            (date!(2026 - 07 - 01), date!(2026 - 07 - 31))
        );
        // IOSS de febrero: hasta el 31 de marzo.
        let d = find(&ds, Model::M369, 2026, Period::Month(2));
        assert_eq!(d.due_on, date!(2026 - 03 - 31));
        // Censo: noviembre y diciembre de 2026, con efectos en 2027; prorrata en el último 303.
        let d = find(&ds, Model::OpcionesNoviembre, 2027, Period::Annual);
        assert_eq!(
            (d.starts_on, d.due_on),
            (date!(2026 - 11 - 01), date!(2026 - 11 - 30))
        );
        let d = find(&ds, Model::OpcionesDiciembre, 2027, Period::Annual);
        assert_eq!(d.due_on, date!(2026 - 12 - 31));
        let d = find(&ds, Model::ProrrataEspecial, 2025, Period::Annual);
        assert_eq!(d.due_on, date!(2026 - 01 - 30));
        // RETA: el cargo es el último día hábil (28/02/2026 es sábado → viernes 27).
        let d = find(&ds, Model::CuotaAutonomos, 2026, Period::Month(2));
        assert_eq!(
            (d.nominal_end, d.due_on),
            (date!(2026 - 02 - 28), date!(2026 - 02 - 27))
        );
        let d = find(&ds, Model::CuotaAutonomos, 2026, Period::Month(4));
        assert_eq!(d.due_on, date!(2026 - 04 - 30));
        // Seguros sociales de diciembre de 2025: hasta el 31/01 (sábado) → viernes 30.
        let d = find(&ds, Model::SegurosSociales, 2025, Period::Month(12));
        assert_eq!(d.due_on, date!(2026 - 01 - 30));
        // Una persona física no tiene IAE.
        assert!(
            !ds.iter()
                .any(|d| matches!(d.model, Model::M840 | Model::M848))
        );
        // 2027 sin orden de la Renta: el segundo plazo es estimado.
        let ds = deadlines_in_year(&todo_marcado_autonomo(), 2027, &HolidayCalendar::default());
        let d = find(&ds, Model::M100SegundoPlazo, 2026, Period::Installment(2));
        assert_eq!(d.due_on, date!(2027 - 11 - 05));
        assert!(d.tentative);
    }

    #[test]
    fn el_369_no_se_traslada_ni_por_festivo_autonomico() {
        // 30/04/2026 festivo inventado: el 303 se movería; el 369 no.
        let cal = HolidayCalendar::new([date!(2026 - 04 - 30)]);
        let ds = deadlines_in_year(&todo_marcado_autonomo(), 2026, &cal);
        let d = find(&ds, Model::M369, 2026, Period::Quarter(1));
        assert_eq!(d.due_on, date!(2026 - 04 - 30));
        let d = find(&ds, Model::CuotaAutonomos, 2026, Period::Month(4));
        assert_eq!(d.due_on, date!(2026 - 04 - 29));
    }

    #[test]
    fn sociedad_232_iae_y_seguros_sociales() {
        let p = todo_marcado_sociedad();
        let ds = deadlines_in_year(&p, 2026, &HolidayCalendar::default());
        let d = find(&ds, Model::M232, 2025, Period::Annual);
        assert_eq!(
            (d.starts_on, d.due_on),
            (date!(2026 - 11 - 01), date!(2026 - 11 - 30))
        );
        // Paga el IAE desde 2027: alta (840) en diciembre de 2026.
        let d = find(&ds, Model::M840, 2027, Period::Annual);
        assert_eq!(
            (d.starts_on, d.due_on),
            (date!(2026 - 12 - 01), date!(2026 - 12 - 31))
        );
        assert!(!ds.iter().any(|d| d.model == Model::M848));
        assert_eq!(
            ds.iter()
                .filter(|d| d.model == Model::SegurosSociales)
                .count(),
            12
        );
        // El administrador autónomo societario: su cuota cada mes y seis cambios de base.
        let count = |m: Model| ds.iter().filter(|d| d.model == m).count();
        assert_eq!(count(Model::CuotaAutonomos), 12);
        assert_eq!(count(Model::CambioBaseReta), 6);
        let d = find(&ds, Model::CambioBaseReta, 2026, Period::Month(2));
        assert_eq!(
            (d.starts_on, d.due_on),
            (date!(2026 - 01 - 01), date!(2026 - 02 - 28))
        );
        // El último bimestre no se mueve aunque el 31/10/2026 sea sábado.
        let d = find(&ds, Model::CambioBaseReta, 2026, Period::Month(10));
        assert_eq!(d.due_on, date!(2026 - 10 - 31));
        // El 840 es una vez: en 2027 ya no sale.
        let ds = deadlines_in_year(&p, 2027, &HolidayCalendar::default());
        assert!(!ds.iter().any(|d| d.model == Model::M840));
        // Ejercicio del 1/7/2025 al 30/06/2026: diez meses hasta el 30/04/2027 → mayo.
        let p = TaxpayerProfile {
            cierre_ejercicio: (Month::June, 30),
            ..todo_marcado_sociedad()
        };
        let ds = deadlines_in_year(&p, 2027, &HolidayCalendar::default());
        let d = find(&ds, Model::M232, 2025, Period::Annual);
        assert_eq!(
            (d.starts_on, d.due_on),
            (date!(2027 - 05 - 01), date!(2027 - 05 - 31))
        );
        // Comunidad de bienes que paga el IAE: 848 del 1/1 al 14/02 (sábado → lunes 16).
        let p = TaxpayerProfile {
            persona: Persona::EntidadAtribucion,
            irpf_method: None,
            iae_desde: Some(2026),
            ..TaxpayerProfile::autonomo()
        };
        let ds = deadlines_in_year(&p, 2026, &HolidayCalendar::default());
        let d = find(&ds, Model::M848, 2024, Period::Annual);
        assert_eq!(d.due_on, date!(2026 - 02 - 16));
        assert!(!ds.iter().any(|d| d.model == Model::M232));
    }

    #[test]
    fn la_cuota_de_autonomos_avisa_una_vez() {
        let ds: Vec<Deadline> =
            deadlines_in_year(&todo_marcado_autonomo(), 2026, &HolidayCalendar::default())
                .into_iter()
                .filter(|d| d.model == Model::CuotaAutonomos && d.period == Period::Month(3))
                .collect();
        let rs = super::super::reminders::reminders(
            &ds,
            &super::super::reminders::DEFAULT_OFFSETS,
            &HolidayCalendar::default(),
            date!(2026 - 01 - 01),
            date!(2026 - 12 - 31),
        );
        // Vence el martes 31/03: un aviso el domingo 29 → viernes 27.
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].date, date!(2026 - 03 - 27));
        assert_eq!(
            super::super::reminders::notification_text(&rs[0]).0,
            "Cuota de autónomos · 03 2026"
        );
    }

    /// Calendario completo de 2027 con los festivos oficiales (BOE-A-2026-21254) de varias
    /// comunidades: todos los perfiles generan sus plazos, cada traslado cae en día hábil y lo que
    /// fija la AEAT cada año (domiciliación, campaña de Renta) va como estimado mientras no se
    /// cargue su calendario del contribuyente de 2027.
    #[test]
    fn calendario_completo_de_2027() {
        use super::super::holidays::{Ccaa, calendar_for};
        for ccaa in [
            None,
            Some(Ccaa::Madrid),
            Some(Ccaa::Galicia),
            Some(Ccaa::Cataluna),
        ] {
            let (cal, cargados) = calendar_for(2027, ccaa, &[]);
            assert!(cargados, "festivos de 2027 cargados");
            for p in [todo_marcado_autonomo(), todo_marcado_sociedad()] {
                let ds = deadlines_in_year(&p, 2027, &cal);
                assert!(ds.len() > 30, "{} plazos", ds.len());
                for d in &ds {
                    assert_eq!(d.due_on.year(), 2027);
                    assert!(
                        d.starts_on <= d.due_on,
                        "{} {}",
                        d.model.code(),
                        d.period.code()
                    );
                    if d.due_on != d.nominal_end {
                        assert!(cal.is_business_day(d.due_on), "{d:?}");
                    } else if !cal.is_business_day(d.due_on) {
                        // Sólo el 369 y los cambios de base del RETA no se trasladan.
                        assert!(
                            matches!(d.model, Model::M369 | Model::CambioBaseReta),
                            "{d:?}"
                        );
                    }
                    assert!(
                        !d.domiciliation_official,
                        "sin calendario AEAT de 2027: {d:?}"
                    );
                }
            }
        }
        let (cal, _) = calendar_for(2027, Some(Ccaa::Madrid), &[]);
        let a = deadlines_in_year(&todo_marcado_autonomo(), 2027, &cal);
        // 30/01/2027 es sábado: el 303 y el 130 del 4T de 2026 pasan al lunes 1 de febrero.
        assert_eq!(
            find(&a, Model::M303, 2026, Period::Quarter(4)).due_on,
            date!(2027 - 02 - 01)
        );
        assert_eq!(
            find(&a, Model::M130, 2026, Period::Quarter(4)).due_on,
            date!(2027 - 02 - 01)
        );
        // 20/04/2027 es martes; el 1T de 2027, sin traslado.
        assert_eq!(
            find(&a, Model::M303, 2027, Period::Quarter(1)).due_on,
            date!(2027 - 04 - 20)
        );
        // Renta 2026: la orden del modelo 100 aún no existe.
        assert!(find(&a, Model::M100, 2026, Period::Annual).tentative);
        let sociedad = TaxpayerProfile {
            obligado_347: true,
            retiene_trabajo_profesionales: true,
            ..todo_marcado_sociedad()
        };
        let s = deadlines_in_year(&sociedad, 2027, &cal);
        // 390 de 2026 hasta el 30/01 (sábado) → 01/02; 190 y 180 hasta el 31/01 (domingo) →
        // 01/02; 347 hasta el 28/02 (domingo) → 01/03; 200 de 2026 hasta el 25/07 (domingo) →
        // 26/07.
        assert_eq!(
            find(&s, Model::M390, 2026, Period::Annual).due_on,
            date!(2027 - 02 - 01)
        );
        assert_eq!(
            find(&s, Model::M190, 2026, Period::Annual).due_on,
            date!(2027 - 02 - 01)
        );
        assert_eq!(
            find(&s, Model::M347, 2026, Period::Annual).due_on,
            date!(2027 - 03 - 01)
        );
        assert_eq!(
            find(&s, Model::M200, 2026, Period::Annual).due_on,
            date!(2027 - 07 - 26)
        );
        // 202 de 2027: 20/04, 20/10 y 20/12 (lunes).
        assert_eq!(
            find(&s, Model::M202, 2027, Period::Installment(3)).due_on,
            date!(2027 - 12 - 20)
        );
    }

    #[test]
    fn traslado_a_habil() {
        // 20/12/2026 es domingo → lunes 21.
        let ds = deadlines_in_year(
            &TaxpayerProfile::sociedad_limitada(),
            2026,
            &HolidayCalendar::default(),
        );
        let d = find(&ds, Model::M202, 2026, Period::Installment(3));
        assert_eq!(d.due_on, date!(2026 - 12 - 21));
    }

    #[test]
    fn add_months_clamps() {
        assert_eq!(add_months(date!(2025 - 12 - 31), 6), date!(2026 - 06 - 30));
        assert_eq!(add_months(date!(2026 - 06 - 30), 1), date!(2026 - 07 - 30));
        assert_eq!(add_months(date!(2025 - 08 - 31), 6), date!(2026 - 02 - 28));
        assert_eq!(add_months(date!(2026 - 01 - 31), -2), date!(2025 - 11 - 30));
        assert_eq!(
            add_months_eom(date!(2026 - 06 - 30), 6),
            date!(2026 - 12 - 31)
        );
        assert_eq!(
            add_months_eom(date!(2026 - 02 - 28), 1),
            date!(2026 - 03 - 31)
        );
        assert_eq!(
            add_months_eom(date!(2026 - 03 - 15), 1),
            date!(2026 - 04 - 15)
        );
    }
}
