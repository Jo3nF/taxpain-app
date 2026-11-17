//! «Preparar la rectificativa» de una autoliquidación ya presentada (carencia G02): 303, 111,
//! 115, 123, 130, 202 y 200.
//!
//! 1. [`anterior`]: lo presentado del periodo (desde TaxPain en producción, lo que tiene la AEAT
//!    según «Comprobar lo presentado» o lo importado o tecleado en «Modelos presentados»), con su
//!    número de justificante y sus casillas.
//! 2. [`preparar`]: recalcula el periodo con la contabilidad de hoy (el borrador de siempre),
//!    pone lado a lado lo presentado y lo que sale ahora casilla a casilla y da el resultado:
//!    - **a ingresar más**: con el recargo por presentar fuera de plazo sin requerimiento (art.
//!      27.2 LGT: 1 % más 1 % por mes completo; pasados doce meses, 15 % e intereses de demora)
//!      calculado con la fecha de presentación prevista;
//!    - **a devolver**: lo ingresado de más, con una estimación de los intereses de demora que
//!      abona la AEAT (art. 32.2 LGT; `TODO(VERIFY)`).
//!
//!    El cauce sale del diseño de registro (`taxpain_models::rectificativa`): autoliquidación
//!    rectificativa en los modelos cuyo DR la trae (303 y 200), complementaria en los demás
//!    (sólo para ingresar más) y, si con la complementaria sale menos, el aviso de que hay que
//!    pedir la rectificación en la Sede.
//! 3. [`exportar`]: el fichero con las marcas, que queda en la auditoría con su huella. Nada se
//!    presenta.
//!
//! Desde cuándo admite cada modelo la rectificativa: [`CALENDARIO`] (datos con su norma).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::rules::add_months;
use taxpain_core::calendar::{Periodicity, Persona, TaxpayerProfile};
use taxpain_core::money::Money;
use taxpain_models::dr::{self, PageWriter};
use taxpain_models::dr_versions;
use taxpain_models::rectificativa::{self as rect, Cauce, Datos};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::models::{self, Draft};

/// Modelos que se rectifican desde Modelos.
pub const MODELOS: [&str; 7] = ["303", "111", "115", "123", "130", "202", "200"];

/// Desde cuándo admite cada modelo la autoliquidación rectificativa, con la norma. Es
/// orientativo: lo que decide el fichero es su diseño de registro. `TODO(VERIFY)` en todos:
/// las fechas no se han cotejado con la Sede; el RD 117/2024 deja que cada orden de aprobación
/// del modelo diga desde cuándo.
pub const CALENDARIO: &[(&str, &str, &str)] = &[
    (
        "303",
        "Desde el 1 de abril de 2025 (el diseño de registro la trae desde el 3T de 2024)",
        "Art. 120.3 LGT (Ley 13/2023), RD 117/2024 y la orden del 303 (DR303e24v200). \
         TODO(VERIFY)",
    ),
    (
        "200",
        "Desde el ejercicio 2025 en el diseño de registro que tiene TaxPain (DR200e25)",
        "Art. 120.3 LGT, RD 117/2024 y la orden del 200 de 2025. TODO(VERIFY)",
    ),
    (
        "111",
        "Aún con complementaria (dr111e16v18)",
        "Arts. 120.3 y 122.2 LGT; art. 126-128 RGAT. TODO(VERIFY)",
    ),
    (
        "115",
        "Aún con complementaria (DR115e15v13)",
        "Arts. 120.3 y 122.2 LGT. TODO(VERIFY)",
    ),
    (
        "123",
        "Aún con complementaria (DR123e24)",
        "Arts. 120.3 y 122.2 LGT. TODO(VERIFY)",
    ),
    (
        "130",
        "Aún con complementaria (DR130e15v12)",
        "Arts. 120.3 y 122.2 LGT. TODO(VERIFY)",
    ),
    (
        "202",
        "Aún con complementaria (DR202e25)",
        "Arts. 120.3 y 122.2 LGT. TODO(VERIFY)",
    ),
];

/// Tipo del interés de demora tributario del año, en millonésimas (40.625 = 4,0625 %): art. 26.6
/// LGT y la ley de presupuestos de cada año, en `taxpain_core::parametros::INTERES_DEMORA` (con
/// su origen: ley del año o prórroga; 2027 sin norma todavía, `TODO(VERIFY)`).
fn tipo_demora(año: i32) -> i64 {
    taxpain_core::parametros::interes_demora(año)
}

fn dias_del_año(y: i32) -> i64 {
    if time::util::is_leap_year(y) {
        366
    } else {
        365
    }
}

/// Intereses de demora de `principal` desde `desde` hasta `hasta` (los dos incluidos), con el
/// tipo de cada año y los días de ese año. `TODO(VERIFY)`: la AEAT redondea cada tramo.
pub fn intereses(principal: Money, desde: Date, hasta: Date) -> Money {
    if principal.cents() <= 0 || hasta < desde {
        return Money::ZERO;
    }
    let mut total = 0i64;
    let mut d = desde;
    loop {
        let y = d.year();
        let fin =
            Date::from_calendar_date(y, time::Month::December, 31).map_or(hasta, |f| f.min(hasta));
        let dias = (fin - d).whole_days() + 1;
        let x = i128::from(principal.cents()) * i128::from(tipo_demora(y)) * i128::from(dias);
        total += redondea(x, y);
        if fin >= hasta {
            break;
        }
        d = fin + Duration::days(1);
    }
    Money::from_cents(total)
}

fn redondea(x: i128, año: i32) -> i64 {
    let div = 1_000_000i128 * i128::from(dias_del_año(año));
    i64::try_from((x + div / 2) / div).unwrap_or(0)
}

/// Recargo del art. 27 LGT de lo que se ingresa ahora.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Recargo27 {
    /// Porcentaje (1 a 15).
    pub porcentaje: i32,
    pub importe: Money,
    /// Meses completos de retraso.
    pub meses: i32,
    /// Más de doce meses: intereses de demora desde el día siguiente a los doce meses.
    pub intereses: Money,
    pub intereses_desde: Option<Date>,
    /// El recargo con la reducción del 25 % del art. 27.5 LGT (si se paga entero en plazo).
    pub reducido: Money,
}

/// Recargo e intereses de presentar el `fecha` lo que vencía el `plazo` (art. 27.2 LGT).
pub fn recargo(plazo: Date, fecha: Date, cuota: Money) -> Option<Recargo27> {
    let r = crate::revision::cruces::plazos::recargo(plazo, fecha, cuota)?;
    let (int, desde) = if r.intereses {
        let desde = add_months(plazo, 12) + Duration::days(1);
        (intereses(cuota, desde, fecha), Some(desde))
    } else {
        (Money::ZERO, None)
    };
    Some(Recargo27 {
        porcentaje: r.tipo.bp() / 100,
        importe: r.importe,
        meses: r.meses,
        intereses: int,
        intereses_desde: desde,
        reducido: r.importe - r.importe.apply_rate(taxpain_core::money::Rate::percent(25)),
    })
}

/// Un modelo presentado que se puede rectificar desde Modelos (el enlace desde otra pantalla).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rectificable {
    pub modelo: String,
    pub ejercicio: i32,
    /// Trimestre (1-4) o pago fraccionado (1-3); 0 en los anuales.
    pub trimestre: u8,
}

/// Lo presentado de un periodo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Anterior {
    /// «taxpain» (presentado desde TaxPain), «aeat» (lo tiene la AEAT según la última
    /// comprobación) o «gestor» (importado o tecleado en «Modelos presentados»).
    pub fuente: String,
    pub justificante: Option<String>,
    pub presentada: Option<Date>,
    /// Tipo de declaración, si se sabe.
    pub declaracion: Option<String>,
    pub casillas: Vec<(u16, Money)>,
    pub resultado: Money,
}

fn fecha(s: &str) -> Option<Date> {
    Date::parse(
        s.get(..10)?,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()
}

fn get(b: &BTreeMap<u16, Money>, k: u16) -> Money {
    b.get(&k).copied().unwrap_or(Money::ZERO)
}

fn check_model(model: &str) -> Result<rect::Liquidacion> {
    if !MODELOS.contains(&model) {
        return Err(AppError::Other(format!(
            "el {model} no se rectifica desde aquí: sólo 303, 111, 115, 123, 130, 202 y 200"
        )));
    }
    rect::liquidacion(model).ok_or_else(|| AppError::Other(format!("modelo {model}")))
}

/// Lo presentado del modelo y periodo, o `None` si no consta presentado.
pub fn anterior(v: &Vault, model: &str, year: i32, quarter: u8) -> Result<Option<Anterior>> {
    let l = check_model(model)?;
    let period = models::period_code(model, quarter);
    let conn = v.connection();
    let resultado = |b: &BTreeMap<u16, Money>| get(b, l.resultado);
    // 1. Presentado desde TaxPain (producción).
    let local: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT declaration, boxes, receipt_number, presented_at FROM tax_return
             WHERE model = ?1 AND year = ?2 AND period = ?3 AND environment = 'produccion'
             ORDER BY presented_at DESC, id DESC LIMIT 1",
            params![model, year, period],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    if let Some((decl, boxes, receipt, at)) = local {
        let list: Vec<(u16, Money)> =
            serde_json::from_str(&boxes).map_err(|e| AppError::Other(e.to_string()))?;
        let b: BTreeMap<u16, Money> = list.into_iter().collect();
        return Ok(Some(Anterior {
            fuente: "taxpain".into(),
            justificante: Some(receipt).filter(|r| !r.trim().is_empty()),
            presentada: fecha(&at),
            declaracion: Some(decl),
            resultado: resultado(&b),
            casillas: b.into_iter().collect(),
        }));
    }
    // 2. Lo que tiene la AEAT (la vigente del periodo) y 3. lo del gestor.
    let vigente = crate::presentado_aeat::vigentes(v)?
        .into_iter()
        .rfind(|x| x.model == model && x.year == year && x.period == period);
    let gestor = crate::presentados::filed_by_gestor(v, model, year, &period)?;
    if vigente.is_none() && gestor.is_none() {
        return Ok(None);
    }
    let de_aeat: Option<BTreeMap<u16, Money>> = vigente
        .as_ref()
        .and_then(|x| x.boxes.as_ref())
        .map(|b| {
            b.casillas
                .iter()
                .filter_map(|(k, m)| Some((u16::try_from(*k).ok()?, *m)))
                .collect::<BTreeMap<u16, Money>>()
        })
        .filter(|b| !b.is_empty());
    let (decl, b) = match (gestor, de_aeat) {
        (Some((d, b)), _) if !b.is_empty() => (d, b),
        (g, Some(b)) => (g.and_then(|(d, _)| d), b),
        (g, None) => (g.and_then(|(d, _)| d), BTreeMap::new()),
    };
    Ok(Some(Anterior {
        fuente: if vigente.is_some() { "aeat" } else { "gestor" }.into(),
        justificante: vigente.as_ref().map(|x| x.receipt.clone()),
        presentada: vigente.as_ref().and_then(|x| fecha(&x.presented_at)),
        declaracion: decl,
        resultado: resultado(&b),
        casillas: b.into_iter().collect(),
    }))
}

/// Lo que elige el usuario.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Opciones {
    /// Número de justificante de la anterior, si TaxPain no lo tiene (o para cambiarlo).
    #[serde(default)]
    pub justificante: Option<String>,
    /// Fecha prevista de presentación (por defecto, hoy).
    #[serde(default)]
    pub fecha: Option<Date>,
    /// Lo que la AEAT ya devolvió (o acordó devolver) de las anteriores ([109] del 303).
    #[serde(default)]
    pub devuelto: Option<Money>,
    /// Pedir la baja (o el cambio) de la domiciliación de la anterior.
    #[serde(default)]
    pub baja_domiciliacion: bool,
}

/// Una casilla: lo presentado y lo de la rectificativa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fila {
    pub casilla: u16,
    pub presentado: Money,
    pub ahora: Money,
    pub diferencia: Money,
}

/// La rectificativa (o complementaria) preparada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rectificativa {
    pub model: String,
    pub year: i32,
    pub quarter: u8,
    pub period: String,
    /// «rectificativa», «complementaria», «otra_via» (pedir la rectificación en la Sede) o
    /// «sin_cambios».
    pub cauce: String,
    /// Qué hacer, en una frase.
    pub cauce_texto: String,
    pub anterior: Anterior,
    pub justificante: Option<String>,
    /// Todas las casillas de las dos, con su diferencia.
    pub filas: Vec<Fila>,
    /// Ingresado con las anteriores (lo que va a la [70], [29], [04]…).
    pub ingresado_antes: Money,
    /// Devuelto o acordado de las anteriores ([109]).
    pub devuelto_antes: Money,
    /// Resultado del periodo con los datos de hoy, antes de descontar lo anterior.
    pub total: Money,
    /// Resultado de la rectificativa ([71], [30]…).
    pub resultado: Money,
    pub a_ingresar: Money,
    /// Lo ingresado de más que se pide devolver ([111]).
    pub a_devolver: Money,
    /// Lo que queda negativo además de eso (a compensar o devolver como en cualquier periodo).
    pub a_compensar: Money,
    pub plazo: Option<Date>,
    pub fecha: Date,
    pub recargo: Option<Recargo27>,
    /// Intereses de demora estimados de lo que se devuelve, hasta la fecha prevista.
    pub intereses_devolucion: Money,
    /// Lo que se paga ahora: lo de más, el recargo y los intereses (el recargo y los intereses
    /// los liquida después la AEAT: no van en el fichero).
    pub total_a_pagar: Money,
    pub declaracion: String,
    pub avisos: Vec<String>,
    pub file_name: String,
    /// El fichero con las marcas (vacío si no se puede generar: ver los avisos).
    pub file: String,
    /// Desde cuándo admite el modelo la rectificativa y su norma.
    pub calendario: Option<(String, String)>,
}

/// Perfil para los plazos: el de la empresa o uno por defecto, con las retenciones y los
/// pagos fraccionados activados (para tener el plazo de todos estos modelos).
fn perfil(v: &Vault) -> Result<TaxpayerProfile> {
    let mut p = match v.setting::<TaxpayerProfile>("taxpayer_profile")? {
        Some(p) => p,
        None => {
            let persona = crate::carpeta::own_nif(v)
                .is_some_and(|n| crate::perfil_deducido::nif_de_persona_fisica(&n));
            if persona {
                TaxpayerProfile::autonomo()
            } else {
                TaxpayerProfile::sociedad_limitada()
            }
        }
    };
    p.retiene_trabajo_profesionales = true;
    p.retiene_alquileres = true;
    p.retiene_capital_mobiliario = true;
    if p.persona == Persona::Juridica {
        p.pagos_fraccionados_is = true;
    }
    if let Some(iva) = p.iva.as_mut() {
        // El borrador es trimestral.
        iva.periodicity = Periodicity::Trimestral;
    }
    Ok(p)
}

/// Último día del plazo voluntario del modelo y periodo.
pub fn plazo(v: &Vault, model: &str, year: i32, period: &str) -> Result<Option<Date>> {
    let p = perfil(v)?;
    Ok(crate::presentado_aeat::expected(&p, year)
        .get(&(model.to_owned(), period.to_owned()))
        .copied())
}

/// Tipo de declaración de lo que se ingresa: domiciliación sólo si ya se domiciliaba y se
/// presenta dentro del plazo.
fn tipo_ingreso(draft: &Draft, fecha: Date, plazo: Option<Date>) -> &'static str {
    if draft.declaration == "U" && plazo.is_some_and(|p| fecha <= p) {
        "U"
    } else {
        "I"
    }
}

/// Prepara la rectificativa (o complementaria) de `model` del periodo `quarter` de `year`.
pub fn preparar(
    v: &Vault,
    model: &str,
    year: i32,
    quarter: u8,
    o: &Opciones,
    hoy: Date,
) -> Result<Rectificativa> {
    let l = check_model(model)?;
    let period = models::period_code(model, quarter);
    let ant = anterior(v, model, year, quarter)?.ok_or_else(|| {
        AppError::Other(format!(
            "no consta presentado el {model} de {}: la rectificativa es de un periodo ya \
             presentado (desde TaxPain, importado en «Modelos presentados» o comprobado con la \
             AEAT)",
            periodo_texto(model, year, &period)
        ))
    })?;
    if ant.casillas.is_empty() {
        return Err(AppError::Other(format!(
            "del {model} presentado de {} no se tienen las casillas: impórtalo en «Comparar con \
             lo presentado» (el fichero o el PDF) o tecléalas, y prepara después la rectificativa",
            periodo_texto(model, year, &period)
        )));
    }
    let fecha = o.fecha.unwrap_or(hoy);
    let draft = models::draft(v, model, year, quarter)?;
    let ahora: BTreeMap<u16, Money> = draft.boxes.iter().copied().collect();
    let prev: BTreeMap<u16, Money> = ant.casillas.iter().copied().collect();
    let mut avisos: Vec<String> = draft.warnings.clone();

    // Lo de las anteriores: lo ingresado ([70] + [71] si salió a ingresar, menos lo que ya se
    // pidió devolver en una rectificativa anterior) y lo devuelto.
    let ingresado = (get(&prev, l.anteriores) + get(&prev, l.resultado).max(Money::ZERO)
        - l.rectificacion.map_or(Money::ZERO, |b| get(&prev, b)))
    .max(Money::ZERO);
    let devuelto = o
        .devuelto
        .unwrap_or_else(|| l.devueltas.map_or(Money::ZERO, |b| get(&prev, b)))
        .max(Money::ZERO);
    if l.devueltas.is_none() && !devuelto.is_zero() {
        avisos.push(format!(
            "El {model} no tiene casilla para lo ya devuelto: no se tiene en cuenta."
        ));
    }
    let devuelto = if l.devueltas.is_some() {
        devuelto
    } else {
        Money::ZERO
    };
    if ant.resultado.cents() < 0
        && ant.declaracion.as_deref() == Some("D")
        && o.devuelto.is_none()
        && l.devueltas.is_some()
    {
        avisos.push(format!(
            "La anterior salió a devolver ({}): si la AEAT ya te la devolvió o la acordó, pon \
             cuánto en «Ya devuelto» (va a la [{}]).",
            ant.resultado.abs(),
            l.devueltas.unwrap_or_default()
        ));
    }
    let total = get(&ahora, l.total.unwrap_or(l.resultado));
    let otros = l.otros.map_or(Money::ZERO, |b| get(&ahora, b));
    let resultado = total - ingresado + devuelto - otros;

    // El cauce: el del diseño del periodo (si TaxPain lo tiene) o el del que genera el fichero.
    let gen_spec = dr_versions::generator_design(model, year, &period).map(|d| d.spec());
    let marcas_gen = gen_spec.and_then(rect::marcas);
    let marcas_periodo = dr_versions::spec_for(model, year, &period).and_then(rect::marcas);
    let cauce_dr = marcas_periodo
        .as_ref()
        .or(marcas_gen.as_ref())
        .map(|m| m.cauce);

    let (a_ingresar, a_devolver, a_compensar) = match cauce_dr {
        Some(Cauce::Rectificativa) if resultado.cents() < 0 => {
            let devolver = (-resultado).min((ingresado - devuelto).max(Money::ZERO));
            (Money::ZERO, devolver, -resultado - devolver)
        }
        _ => (resultado.max(Money::ZERO), Money::ZERO, Money::ZERO),
    };

    // Casillas de la rectificativa: las de hoy con lo de las anteriores y el resultado nuevo.
    let mut nuevas = ahora.clone();
    nuevas.insert(l.anteriores, ingresado);
    if let Some(b) = l.devueltas {
        nuevas.insert(b, devuelto);
    }
    if let Some(b) = l.rectificacion {
        nuevas.insert(b, a_devolver);
    }
    nuevas.insert(l.resultado, resultado);
    if model == "200" {
        // [01586] es el resultado antes de lo anterior; [00621], el de la rectificativa.
        nuevas.insert(1586, total);
    }
    let claves: BTreeSet<u16> = prev.keys().chain(nuevas.keys()).copied().collect();
    let filas: Vec<Fila> = claves
        .into_iter()
        .map(|k| {
            let (p, n) = (get(&prev, k), get(&nuevas, k));
            Fila {
                casilla: k,
                presentado: p,
                ahora: n,
                diferencia: n - p,
            }
        })
        .filter(|f| !(f.presentado.is_zero() && f.ahora.is_zero()))
        .collect();
    // ¿Cambia algo de lo declarado? (Sin contar las casillas de lo anterior, que sólo
    // existen en la rectificativa.)
    let propias: BTreeSet<u16> = [Some(l.anteriores), l.devueltas, l.rectificacion]
        .into_iter()
        .flatten()
        .collect();
    let cambia_casillas = filas.iter().any(|f| {
        !f.diferencia.is_zero() && !propias.contains(&f.casilla) && f.casilla != l.resultado
    });
    let sin_cambios = !cambia_casillas && resultado.is_zero() && a_devolver.is_zero();

    let plazo_dia = plazo(v, model, year, &period)?;
    if plazo_dia.is_none() {
        avisos.push(format!(
            "TaxPain no sabe el último día del plazo del {model} de {}: no calcula el recargo.",
            periodo_texto(model, year, &period)
        ));
    }
    let recargo = match plazo_dia {
        Some(p) if a_ingresar.cents() > 0 => recargo(p, fecha, a_ingresar),
        _ => None,
    };
    // Intereses de lo que se devuelve: desde que se ingresó (el último día del plazo si se
    // domicilió) hasta la fecha prevista; la AEAT los abona hasta que ordena el pago.
    let intereses_devolucion = if a_devolver.cents() > 0 {
        let desde = match (ant.declaracion.as_deref(), ant.presentada) {
            (Some("U"), _) => plazo_dia,
            (_, Some(f)) => Some(f),
            (_, None) => plazo_dia,
        };
        match desde {
            Some(d) => intereses(a_devolver, d + Duration::days(1), fecha),
            None => Money::ZERO,
        }
    } else {
        Money::ZERO
    };

    let calendario = CALENDARIO
        .iter()
        .find(|(m, _, _)| *m == model)
        .map(|(_, desde, norma)| ((*desde).to_owned(), (*norma).to_owned()));
    let (cauce, cauce_texto) = if sin_cambios {
        (
            "sin_cambios",
            "Con los datos de hoy sale lo mismo que lo presentado: no hay nada que rectificar."
                .to_owned(),
        )
    } else {
        match cauce_dr {
            Some(Cauce::Rectificativa) => (
                "rectificativa",
                if a_ingresar.cents() > 0 {
                    format!(
                        "Autoliquidación rectificativa: sale {a_ingresar} más a ingresar (art. \
                         120.3 LGT y RD 117/2024)."
                    )
                } else if a_devolver.cents() > 0 {
                    format!(
                        "Autoliquidación rectificativa: se pide la devolución de {a_devolver} \
                         ingresados de más (art. 120.3 LGT y RD 117/2024)."
                    )
                } else {
                    "Autoliquidación rectificativa: cambian las casillas pero no lo ingresado \
                     (art. 120.3 LGT y RD 117/2024)."
                        .to_owned()
                },
            ),
            Some(Cauce::Complementaria) if resultado.cents() > 0 => (
                "complementaria",
                format!(
                    "Declaración complementaria: sale {a_ingresar} más a ingresar (art. 122.2 \
                     LGT). El diseño de registro del {model} aún no trae la autoliquidación \
                     rectificativa."
                ),
            ),
            Some(Cauce::Complementaria) if resultado.cents() < 0 => (
                "otra_via",
                format!(
                    "Sale {} menos de lo ingresado: la complementaria sólo sirve para ingresar \
                     más. Pide la rectificación de la autoliquidación en la Sede de la AEAT \
                     (art. 120.3 LGT; arts. 126 a 128 RGAT) para que te devuelvan lo ingresado \
                     de más, con sus intereses (art. 32 LGT). TODO(VERIFY): si la AEAT ya admite \
                     la rectificativa del {model}, hazla en el formulario de la Sede.",
                    -resultado
                ),
            ),
            Some(Cauce::Complementaria) => (
                "otra_via",
                format!(
                    "Lo ingresado no cambia, pero sí alguna casilla: una complementaria no \
                     tendría nada que ingresar. Pide la rectificación en la Sede (art. 120.3 \
                     LGT) o, si sólo cambian bases o perceptores, corrígelo en el resumen anual. \
                     TODO(VERIFY) con la AEAT para el {model}."
                ),
            ),
            None => (
                "otra_via",
                format!(
                    "TaxPain no tiene en el diseño de registro del {model} las marcas de \
                     rectificativa ni de complementaria (TODO(VERIFY)): haz la corrección en el \
                     formulario de la Sede con las casillas de abajo."
                ),
            ),
        }
    };

    let declaracion = match (cauce, model) {
        (_, _) if a_ingresar.cents() > 0 => tipo_ingreso(&draft, fecha, plazo_dia).to_owned(),
        ("rectificativa", "303") if a_compensar.is_zero() => "N".to_owned(),
        ("rectificativa", "303") => match draft.declaration.as_str() {
            d @ ("C" | "D") => d.to_owned(),
            _ => "C".to_owned(),
        },
        ("rectificativa", "200") if a_compensar.is_zero() => "N".to_owned(),
        ("rectificativa", "200") => "D".to_owned(),
        _ => draft.declaration.clone(),
    };

    // Justificante de la anterior.
    let justificante = o
        .justificante
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| ant.justificante.clone());
    let mut file = String::new();
    let generar = matches!(cauce, "rectificativa" | "complementaria");
    if generar {
        match (&justificante, gen_spec, &marcas_gen) {
            (None, _, _) => avisos.push(
                "Falta el número de justificante de la anterior (trece cifras, en su \
                 justificante de presentación): escríbelo para generar el fichero."
                    .into(),
            ),
            (Some(j), _, _) if !rect::justificante_valido(j) => avisos.push(format!(
                "El número de justificante «{j}» no tiene trece cifras: corrígelo para generar \
                 el fichero."
            )),
            (_, None, _) | (_, _, None) => avisos.push(format!(
                "El diseño de registro con que TaxPain genera el {model} no trae las marcas de \
                 {cauce} (TODO(VERIFY)): no se genera el fichero; presenta con el formulario de \
                 la Sede y las casillas de abajo."
            )),
            (Some(_), Some(_), Some(m)) if nombre(m.cauce) != cauce => avisos.push(format!(
                "Para este periodo toca una {cauce}, pero el diseño con que TaxPain genera el \
                 fichero sólo trae la {}: no se genera el fichero (TODO(VERIFY)); presenta con \
                 el formulario de la Sede.",
                nombre(m.cauce)
            )),
            (Some(_), _, _) if draft.file.is_empty() => avisos.push(format!(
                "El {model} de este periodo no tiene fichero en TaxPain: copia las casillas en \
                 el formulario de la Sede."
            )),
            (Some(j), Some(spec), Some(_)) => {
                let datos = Datos {
                    justificante: j.clone(),
                    tipo: declaracion.clone(),
                    anteriores: ingresado,
                    devueltas: devuelto,
                    resultado,
                    rectificacion: a_devolver,
                    baja_domiciliacion: o.baja_domiciliacion,
                };
                match rect::marcar(model, spec, &draft.file, &datos) {
                    Ok(f) => file = f,
                    Err(e) => avisos.push(format!("No se ha podido generar el fichero: {e}")),
                }
            }
        }
    }
    // 303 con devolución de la rectificación: los datos del banco aunque no sea «devolución»
    // (DR303e26v101, página DID), salvo que se pida la baja de la domiciliación.
    if model == "303" && !file.is_empty() && a_devolver.cents() > 0 && !o.baja_domiciliacion {
        let s = models::settings(v)?;
        let has_did = file.contains("<T303DID00>");
        match (&s.iban, has_did, gen_spec) {
            (_, true, _) => {}
            (Some(iban), false, Some(spec)) => {
                let mut w = PageWriter::new(spec.page("DID")?);
                w.text(6, iban)?;
                file = dr::append_page(&file, &w.finish()?);
            }
            _ => avisos.push(format!(
                "Para que te devuelvan los {a_devolver} de la [111] hace falta el IBAN: ponlo en \
                 «Datos de los modelos»."
            )),
        }
    }
    if model == "200" && cauce == "rectificativa" {
        avisos.push(
            "200: comprueba en Sociedades WEB la casilla de la rectificación [00866] y los \
             importes (A) y (B) de la página de la rectificativa, que TaxPain deja en cero \
             (TODO(VERIFY): el DR no explica cómo se rellenan)."
                .into(),
        );
    }
    if model == "303" && cauce == "rectificativa" && a_devolver.cents() > 0 {
        avisos.push(
            "303: TODO(VERIFY) el tipo de declaración y la [111] cuando la rectificativa sale \
             negativa (el DR dice que el tipo corresponde a la [71] sin la parte de la [111])."
                .into(),
        );
    }
    if model == "303" && !sin_cambios && (resultado.cents() < 0 || ant.resultado.cents() < 0) {
        avisos.push(format!(
            "Lo que queda a compensar cambia: revisa la [110] de los 303 siguientes a {}.",
            periodo_texto(model, year, &period)
        ));
    }
    if matches!(model, "111" | "115" | "123") && !sin_cambios {
        avisos.push(format!(
            "Revisa después el resumen anual del {model} ({}).",
            match model {
                "111" => "190",
                "115" => "180",
                _ => "193",
            }
        ));
    }
    if let Some(r) = &recargo {
        avisos.push(format!(
            "Fuera de plazo y sin requerimiento: recargo del {} % ({}){} (art. 27.2 LGT). No va \
             en el fichero: lo liquida después la AEAT; si lo pagas entero en el plazo de esa \
             liquidación, se reduce un 25 % ({}, art. 27.5 LGT).",
            r.porcentaje,
            r.importe,
            if r.intereses.is_zero() {
                String::new()
            } else {
                format!(" más {} de intereses de demora", r.intereses)
            },
            r.reducido
        ));
    }
    if a_devolver.cents() > 0 {
        avisos.push(format!(
            "La AEAT devuelve lo ingresado de más con intereses de demora desde que se ingresó \
             (art. 32.2 LGT): hasta el {} serían unos {} (TODO(VERIFY): el cómputo exacto lo \
             hace la AEAT al pagar).",
            dia(fecha),
            intereses_devolucion
        ));
    }
    let total_a_pagar = a_ingresar
        + recargo
            .as_ref()
            .map_or(Money::ZERO, |r| r.importe + r.intereses);
    let tipo = if cauce == "complementaria" {
        "complementaria"
    } else {
        "rectificativa"
    };
    let file_name = draft.file_name.strip_suffix(".txt").map_or_else(
        || format!("{}_{tipo}.txt", draft.file_name),
        |b| format!("{b}_{tipo}.txt"),
    );
    Ok(Rectificativa {
        model: model.to_owned(),
        year,
        quarter,
        period,
        cauce: cauce.to_owned(),
        cauce_texto,
        anterior: ant,
        justificante,
        filas,
        ingresado_antes: ingresado,
        devuelto_antes: devuelto,
        total,
        resultado,
        a_ingresar,
        a_devolver,
        a_compensar,
        plazo: plazo_dia,
        fecha,
        recargo,
        intereses_devolucion,
        total_a_pagar,
        declaracion,
        avisos,
        file_name,
        file,
        calendario,
    })
}

fn nombre(c: Cauce) -> &'static str {
    match c {
        Cauce::Rectificativa => "rectificativa",
        Cauce::Complementaria => "complementaria",
    }
}

/// «20/04/2026».
fn dia(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// «3T de 2026», «2P de 2026» o «2026».
fn periodo_texto(model: &str, year: i32, period: &str) -> String {
    if model == "200" || period == "0A" {
        year.to_string()
    } else {
        format!("{period} de {year}")
    }
}

/// Bytes del fichero de la rectificativa (ISO-8859-1), con la huella en la auditoría. Nada se
/// presenta.
pub fn exportar(v: &Vault, r: &Rectificativa, now: OffsetDateTime) -> Result<Vec<u8>> {
    if r.file.is_empty() {
        return Err(AppError::Other(format!(
            "no hay fichero de la {}: {}",
            if r.cauce == "complementaria" {
                "complementaria"
            } else {
                "rectificativa"
            },
            r.avisos
                .iter()
                .find(|a| a.contains("fichero"))
                .cloned()
                .unwrap_or_else(|| r.cauce_texto.clone())
        )));
    }
    let bytes = dr::to_latin1(&r.file)?;
    use sha2::{Digest, Sha256};
    audit::append(
        v.connection(),
        &crate::invoicing::now_str(now),
        Actor::User,
        "models.rectificativa.export",
        &format!("{}/{}/{}", r.model, r.year, r.period),
        &serde_json::json!({
            "cauce": r.cauce,
            "previous_receipt": r.justificante,
            "sha256": hex::encode(Sha256::digest(&bytes)),
            "result": r.resultado.cents(),
            "to_pay": r.a_ingresar.cents(),
            "to_refund": r.a_devolver.cents(),
            "surcharge": r.recargo.as_ref().map(|x| x.importe.cents()),
            "planned_date": r.fecha.to_string(),
        }),
    )?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    #[test]
    fn recargo_por_meses_completos() {
        let plazo = date!(2026 - 04 - 20);
        // Menos de un mes: 1 %.
        let r = recargo(plazo, date!(2026 - 05 - 19), eur(1_000)).unwrap();
        assert_eq!((r.porcentaje, r.importe, r.meses), (1, eur(10), 0));
        // Tres meses completos: 4 %.
        let r = recargo(plazo, date!(2026 - 07 - 25), eur(1_000)).unwrap();
        assert_eq!((r.porcentaje, r.importe, r.meses), (4, eur(40), 3));
        assert_eq!(r.reducido, eur(30));
        assert!(r.intereses.is_zero());
        // Once meses: 12 %; doce o más (pasado el año): 15 % e intereses.
        let r = recargo(plazo, date!(2027 - 04 - 10), eur(1_000)).unwrap();
        assert_eq!((r.porcentaje, r.meses), (12, 11));
        let r = recargo(plazo, date!(2027 - 06 - 20), eur(1_000)).unwrap();
        assert_eq!(r.porcentaje, 15);
        assert_eq!(r.intereses_desde, Some(date!(2027 - 04 - 21)));
        // 61 días al 4,0625 %: 1.000 × 0,040625 × 61 / 365 = 6,79.
        assert_eq!(r.intereses, Money::from_cents(679));
        // En plazo, nada.
        assert!(recargo(plazo, plazo, eur(1_000)).is_none());
    }

    #[test]
    fn intereses_por_tramos_de_año() {
        // Del 1 de diciembre de 2022 al 31 de enero de 2023: 31 días al 3,75 % y 31 al 4,0625 %.
        let i = intereses(eur(10_000), date!(2022 - 12 - 01), date!(2023 - 01 - 31));
        // 10.000 × 0,0375 × 31/365 = 31,85; 10.000 × 0,040625 × 31/365 = 34,50.
        assert_eq!(i, Money::from_cents(3185 + 3450));
        assert!(intereses(eur(100), date!(2026 - 02 - 01), date!(2026 - 01 - 01)).is_zero());
    }
}
