//! Informe del ejercicio: posiciones valoradas al cierre, resultado de las ventas, dividendos,
//! intereses y retenciones, la deducción por doble imposición internacional (sólo
//! informativa), el modelo 720 y los asientos propuestos.
//!
//! Normas (leídas en el BOE el 2026-11-02):
//! - **Deducción por doble imposición jurídica internacional, art. 31 LIS** (Ley 27/2014): se
//!   deduce «la menor de las dos cantidades siguientes: a) El importe efectivo de lo satisfecho
//!   en el extranjero por razón del gravamen de naturaleza idéntica o análoga a este Impuesto
//!   [...] Siendo de aplicación un convenio para evitar la doble imposición, la deducción no
//!   podrá exceder del impuesto que corresponda según aquél. b) El importe de la cuota íntegra
//!   que en España correspondería pagar por las mencionadas rentas si se hubieran obtenido en
//!   territorio español»; por países (31.3); el impuesto extranjero «se incluirá en la renta»
//!   aunque no sea plenamente deducible (31.2) y lo no deducido por falta de cuota se puede
//!   deducir en los periodos siguientes (31.6). El exceso no deducible sólo es gasto si
//!   corresponde a actividades económicas en el extranjero (31.2): en dividendos de cartera, no.
//!   Persona física: art. 80 LIRPF (mismo esquema con el tipo medio). Convenio España–EE. UU.
//!   (BOE-A-1990-30940, art. 10.2.b): 15 % para dividendos de cartera.
//! - **Modelo 720** (art. 42 ter RGAT, RD 1065/2007; DA 18.ª LGT): valores depositados o
//!   situados en el extranjero a 31 de diciembre; no hay que informar si con las IIC y los
//!   seguros «no superen, conjuntamente, el importe de 50.000 euros» (42 ter.4.c); plazo, del 1
//!   de enero al 31 de marzo; los años siguientes, sólo si el conjunto sube más de 20.000 €
//!   (42 ter.5). **Las personas jurídicas residentes que «tengan registrados en su contabilidad
//!   de forma individualizada» esos valores no están obligadas** (42 ter.4.b; la FAQ de la AEAT
//!   «Criterio de exoneración por contabilización» lo admite en sentido amplio). El 721 es sólo
//!   de monedas virtuales (art. 42 quater RGAT).

use std::collections::BTreeMap;

use serde::Serialize;
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use time::{Date, Month};

use super::calculo::{self, Metodo, Mov, Movimiento};
use super::{
    Cartera, Clase, Cuenta, Op, Settings, TipoOp, Valor, a_euros, cambio_a, cuentas, ops, precio_a,
    settings, valores,
};
use crate::error::Result;

/// Valores en el extranjero a 31/12 que obligan al 720 (art. 42 ter.4.c RGAT).
pub const UMBRAL_720: Money = Money::from_euros(50_000, 0);

/// Tipo de retención en origen sobre dividendos de cartera que permite el convenio con cada
/// país (y si está comprobado en el BOE). `TODO(VERIFY)` los no comprobados.
pub const CONVENIOS: &[(&str, u8, bool)] = &[
    ("US", 15, true),
    ("DE", 15, false),
    ("FR", 15, false),
    ("NL", 15, false),
    ("CH", 15, false),
    ("IT", 15, false),
    ("BE", 15, false),
    ("CA", 15, false),
    ("PT", 15, false),
    ("AT", 15, false),
    ("SE", 15, false),
    ("NO", 15, false),
    ("IE", 15, false),
    ("LU", 15, false),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PosicionView {
    pub security_id: i64,
    pub ticker: String,
    pub name: String,
    pub isin: String,
    pub kind: Clase,
    pub account_id: i64,
    pub account: String,
    pub abroad: bool,
    pub country: String,
    pub cantidad: Qty,
    pub coste: Money,
    /// Precio de cierre usado, su divisa y su fecha (si es anterior a la del informe).
    pub precio: Option<Precio>,
    pub divisa: String,
    pub precio_fecha: Option<Date>,
    /// Euros por unidad de la divisa.
    pub cambio: Option<Precio>,
    pub valor: Option<Money>,
    pub latente: Option<Money>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RealizadoView {
    pub op: i64,
    pub fecha: Date,
    pub security_id: i64,
    pub ticker: String,
    pub name: String,
    pub account: String,
    pub cantidad: Qty,
    pub transmision: Money,
    pub coste: Money,
    pub resultado: Money,
    /// Persona física: pérdida con compras del mismo valor en los dos meses anteriores o
    /// posteriores (art. 33.5.f LIRPF): no se computa todavía.
    pub dos_meses: bool,
    /// Venta o vencimiento de una opción (u otro derivado): va en la línea de derivados.
    pub derivado: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Resultado {
    /// Ventas de valores (sin las de derivados).
    pub ventas: usize,
    pub ganancias: Money,
    pub perdidas: Money,
    /// Ganancias − pérdidas.
    pub neto: Money,
    /// Resultado de las opciones y otros derivados (primas que vencen, cierres), aparte.
    pub derivados: Money,
    pub ventas_derivados: usize,
    /// Dividendos íntegros (antes de retenciones).
    pub dividendos: Money,
    pub retenciones_extranjero: Money,
    pub retenciones_espana: Money,
    /// Intereses (cobrados menos pagados).
    pub intereses: Money,
    /// Comisiones que van a resultados (las de la cuenta y, en negociación, las de compraventa).
    pub comisiones: Money,
    pub impuestos: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RentaPais {
    pub pais: String,
    /// Dividendos íntegros de valores de ese país.
    pub rentas: Money,
    pub retenido: Money,
    /// Tipo del convenio para dividendos de cartera, si se conoce, y si está comprobado.
    pub convenio_pct: Option<u8>,
    pub convenio_comprobado: bool,
    /// Retención que admite el convenio (tipo × rentas).
    pub tope_convenio: Option<Money>,
    /// Cuota que correspondería en España (tipo × rentas).
    pub cuota_espanola: Money,
    pub deducible: Money,
    /// Retenido por encima del convenio: se puede pedir la devolución al otro país.
    pub exceso: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DobleImposicion {
    /// Tipo con el que se calcula la cuota española (25 % general en el IS, art. 29.1 LIS; 19 %
    /// del primer tramo del ahorro en el IRPF).
    pub tipo_pct: u8,
    pub paises: Vec<RentaPais>,
    pub retenido: Money,
    pub deducible: Money,
    pub nota: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct M720Cuenta {
    pub account: String,
    pub country: String,
    pub valor: Money,
    /// (ticker, ISIN, títulos, valor en euros).
    pub valores: Vec<(String, String, Qty, Money)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct M720 {
    pub year: i32,
    pub fecha: Date,
    pub total_extranjero: Money,
    pub umbral: Money,
    pub supera: bool,
    pub cuentas: Vec<M720Cuenta>,
    pub sin_precio: Vec<String>,
    pub persona_fisica: bool,
    /// Sociedad: no presenta el 720 si los tiene contabilizados de forma individualizada.
    pub exenta_por_contabilidad: bool,
    pub nota: String,
}

/// Una cuenta con lo que se sabe de su conector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuentaView {
    #[serde(flatten)]
    pub cuenta: Cuenta,
    pub ops: usize,
    /// `api` (IBKR con token), `pdf` (XTB), `fichero` (IBKR sin token) o `manual`.
    pub conector: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub year: i32,
    pub inicio: Date,
    pub fecha: Date,
    pub persona_fisica: bool,
    pub metodo: Metodo,
    pub settings: Settings,
    pub cuentas: Vec<CuentaView>,
    pub posiciones: Vec<PosicionView>,
    pub coste_total: Money,
    pub valor_total: Money,
    pub latente_total: Money,
    pub resultado: Resultado,
    pub realizados: Vec<RealizadoView>,
    pub doble_imposicion: DobleImposicion,
    pub m720: M720,
    pub asientos: Vec<super::asientos::Propuesto>,
    pub ops_ejercicio: usize,
    /// Operaciones del ejercicio en otra divisa aún sin euros («Pasar a euros con el BCE»).
    pub sin_euros: usize,
    pub avisos: Vec<String>,
    pub notas: Vec<String>,
}

fn movimiento(o: &Op) -> Option<Mov> {
    let tipo = match o.kind {
        TipoOp::Compra => Movimiento::Compra,
        TipoOp::Venta => Movimiento::Venta,
        TipoOp::Traspaso => Movimiento::Traspaso,
        TipoOp::Split => Movimiento::Split,
        _ => return None,
    };
    Some(Mov {
        id: o.id,
        fecha: o.date,
        hora: o.time.clone(),
        tipo,
        valor: o.security_id?,
        cuenta: o.account_id,
        destino: o.to_account_id,
        cantidad: o.qty,
        euros: o.eur,
        comision: o.fee_eur,
    })
}

/// Inicio y fin del ejercicio (el de la contabilidad o el año natural).
pub(crate) fn ejercicio(v: &Vault, year: i32) -> (Date, Date) {
    let natural = (
        Date::from_calendar_date(year, Month::January, 1).unwrap_or(Date::MIN),
        Date::from_calendar_date(year, Month::December, 31).unwrap_or(Date::MIN),
    );
    crate::accounting::fiscal_year(v, year)
        .map(|f| (f.start, f.end))
        .unwrap_or(natural)
}

pub(crate) fn nombre_valor(vs: &[Valor], id: i64) -> String {
    vs.iter()
        .find(|x| x.id == id)
        .map(|x| x.ticker.clone())
        .unwrap_or_else(|| format!("valor {id}"))
}

/// Posiciones a `fecha` valoradas con los precios de cierre y los cambios.
fn posiciones_a(
    v: &Vault,
    movs: &[Mov],
    metodo: Metodo,
    al_coste: bool,
    fecha: Date,
    vs: &[Valor],
    cs: &[Cuenta],
) -> Result<(Vec<PosicionView>, Vec<String>)> {
    let nombre = |id: i64| nombre_valor(vs, id);
    let c = calculo::calcular(movs, metodo, al_coste, Some(fecha), &nombre);
    let mut out = Vec::new();
    for p in &c.posiciones {
        let x = vs.iter().find(|x| x.id == p.valor);
        let cuenta = cs.iter().find(|x| x.id == p.cuenta);
        let precio = precio_a(v, p.valor, fecha)?;
        let divisa = precio
            .as_ref()
            .map(|(_, _, d)| d.clone())
            .or_else(|| x.map(|x| x.currency.clone()))
            .unwrap_or_else(|| "EUR".into());
        let cambio = cambio_a(v, &divisa, fecha)?.map(|(_, f)| f);
        let valor = match (&precio, cambio) {
            (Some((_, pr, _)), Some(fx)) => p.cantidad.valor(*pr).map(|m| a_euros(m, fx)),
            _ => None,
        };
        out.push(PosicionView {
            security_id: p.valor,
            ticker: x.map(|x| x.ticker.clone()).unwrap_or_default(),
            name: x.map(|x| x.name.clone()).unwrap_or_default(),
            isin: x.map(|x| x.isin.clone()).unwrap_or_default(),
            kind: x.map(|x| x.kind).unwrap_or_default(),
            account_id: p.cuenta,
            account: cuenta.map(|x| x.name.clone()).unwrap_or_default(),
            abroad: cuenta.is_some_and(|x| x.abroad),
            country: cuenta.map(|x| x.country.clone()).unwrap_or_default(),
            cantidad: p.cantidad,
            coste: p.coste,
            precio: precio.as_ref().map(|(_, pr, _)| *pr),
            divisa,
            precio_fecha: precio.as_ref().map(|(d, _, _)| *d).filter(|d| *d != fecha),
            cambio,
            valor,
            latente: valor.map(|x| x - p.coste),
        });
    }
    Ok((out, c.avisos))
}

/// Añade a `movs`, en cada cierre de ejercicio anterior a `inicio`, la valoración de cada valor a
/// su valor razonable (precio de cierre por cambio). Devuelve los avisos de los que no se pueden
/// valorar (sin precio o sin cambio): ésos siguen a coste.
#[allow(clippy::too_many_arguments)]
fn valorar_cierres_anteriores(
    v: &Vault,
    movs: &mut Vec<Mov>,
    metodo: Metodo,
    al_coste: bool,
    year: i32,
    inicio: Date,
    vs: &[Valor],
    cs: &[Cuenta],
) -> Result<Vec<String>> {
    let mut avisos = Vec::new();
    let Some(primero) = movs.iter().map(|m| m.fecha).min() else {
        return Ok(avisos);
    };
    let mut n = 0i64;
    for y in primero.year()..year {
        let (_, fin) = ejercicio(v, y);
        if fin < primero || fin >= inicio {
            continue;
        }
        let (pos, _) = posiciones_a(v, movs, metodo, al_coste, fin, vs, cs)?;
        let mut por: BTreeMap<i64, Option<Money>> = BTreeMap::new();
        for p in pos.iter().filter(|p| p.cantidad > Qty::ZERO) {
            let e = por.entry(p.security_id).or_insert(Some(Money::ZERO));
            *e = e.zip(p.valor).map(|(a, b)| a + b);
        }
        for (valor, total) in por {
            match total {
                Some(euros) => {
                    n += 1;
                    movs.push(Mov {
                        id: -n,
                        fecha: fin,
                        hora: "24:00:00".into(),
                        tipo: Movimiento::Valoracion,
                        valor,
                        cuenta: 0,
                        destino: None,
                        cantidad: Qty::ZERO,
                        euros,
                        comision: Money::ZERO,
                    });
                }
                None => avisos.push(format!(
                    "{}: sin precio de cierre o sin cambio a {fin}; se sigue a coste desde \
                     entonces (el ajuste a valor razonable de este año no descuenta el de aquel \
                     cierre).",
                    nombre_valor(vs, valor)
                )),
            }
        }
    }
    Ok(avisos)
}

fn m720_de(year: i32, fecha: Date, posiciones: &[PosicionView], fisica: bool) -> M720 {
    let mut por: BTreeMap<String, M720Cuenta> = BTreeMap::new();
    let mut sin_precio = Vec::new();
    // Las opciones no son valores del 720 (art. 42 ter RGAT: valores, IIC y seguros).
    for p in posiciones
        .iter()
        .filter(|p| p.abroad && p.kind != Clase::Derivado)
    {
        let e = por.entry(p.account.clone()).or_insert_with(|| M720Cuenta {
            account: p.account.clone(),
            country: p.country.clone(),
            valor: Money::ZERO,
            valores: Vec::new(),
        });
        let val = p.valor.unwrap_or(Money::ZERO);
        e.valor += val;
        e.valores
            .push((p.ticker.clone(), p.isin.clone(), p.cantidad, val));
        if p.valor.is_none() && !sin_precio.contains(&p.ticker) {
            sin_precio.push(p.ticker.clone());
        }
    }
    let total: Money = por.values().map(|c| c.valor).sum();
    let supera = total > UMBRAL_720;
    let nota = if por.is_empty() {
        "No hay valores en cuentas marcadas como del extranjero a 31 de diciembre: el 720 de \
         valores no toca. Si una cuenta de fuera no está marcada así, edítala en «Cuentas»."
            .to_owned()
    } else if !sin_precio.is_empty() {
        format!(
            "Faltan precios a {fecha} de {}: la valoración está incompleta. El 720 valora los \
             valores cotizados a su cotización a 31 de diciembre y las IIC (fondos, y ETF \
             `TODO(VERIFY)`) a su valor liquidativo.",
            sin_precio.join(", ")
        )
    } else if fisica && supera {
        format!(
            "Los valores en el extranjero a {fecha} suman {total}, más de 50.000 € (límite \
             conjunto de valores, IIC y seguros, art. 42 ter.4.c RGAT): toca el 720 de {year} \
             entre el 1 de enero y el 31 de marzo de {}. Los años siguientes, sólo si el conjunto \
             sube más de 20.000 € o se deja de ser titular. El 721 no aplica (es de monedas \
             virtuales).",
            year + 1
        )
    } else if supera {
        format!(
            "Los valores en el extranjero a {fecha} suman {total}, más de 50.000 €. Una \
             sociedad residente que los «tenga registrados en su contabilidad de forma \
             individualizada» no tiene que presentar el 720 (art. 42 ter.4.b RGAT; FAQ de la \
             AEAT «Criterio de exoneración por contabilización»): contabilizados aquí, cada valor \
             con su subcuenta, su ISIN, el bróker y el país, lo cumples. El 721 no aplica (es de \
             monedas virtuales)."
        )
    } else {
        format!(
            "Los valores en el extranjero a {fecha} suman {total}: no llegan a 50.000 €; con las \
             IIC y los seguros que tengas fuera, el límite es conjunto (art. 42 ter.4.c RGAT). \
             {}",
            if fisica {
                "Si no se supera, no hay 720 de valores."
            } else {
                "Además, una sociedad que los tiene contabilizados de forma individualizada no \
                 presenta el 720 (art. 42 ter.4.b RGAT)."
            }
        )
    };
    // Un precio de antes del 31 de diciembre (el último que hay) no es la cotización a esa fecha.
    let viejos: Vec<&str> = posiciones
        .iter()
        .filter(|p| p.abroad && p.precio_fecha.is_some())
        .map(|p| p.ticker.as_str())
        .collect();
    let nota = if viejos.is_empty() {
        nota
    } else {
        format!(
            "{nota} Ojo: {} se valoran con un precio anterior al {fecha}: pon el de esa fecha en \
             «Precios de cierre».",
            viejos.join(", ")
        )
    };
    M720 {
        year,
        fecha,
        total_extranjero: total,
        umbral: UMBRAL_720,
        supera,
        cuentas: por.into_values().collect(),
        sin_precio,
        persona_fisica: fisica,
        exenta_por_contabilidad: !fisica,
        nota,
    }
}

/// País de una retención o un dividendo: el de la operación o, si no lo lleva, el del valor (o
/// el de su ISIN). Lo usan el resumen, la doble imposición y los asientos (473 frente a 6300).
pub(crate) fn pais_de(o: &Op, vs: &[Valor]) -> String {
    if o.country.len() == 2 {
        return o.country.to_uppercase();
    }
    o.security_id
        .and_then(|id| vs.iter().find(|x| x.id == id))
        .map(|x| {
            if x.country.len() == 2 {
                x.country.to_uppercase()
            } else {
                x.isin.get(..2).unwrap_or("").to_uppercase()
            }
        })
        .unwrap_or_default()
}

/// Tipo con el que se calcula la cuota española de las rentas de fuera: en el IRPF, el 19 % del
/// primer tramo del ahorro; en el IS, el de la sociedad en el ejercicio (régimen de «Datos de los
/// modelos», art. 29.1 LIS) y, en las de menos de 1 M€ de cifra de negocios, el del primer tramo.
fn tipo_cuota(v: &Vault, year: i32, fisica: bool) -> u8 {
    use taxpain_models::m200::TaxRegime;
    if fisica {
        return 19;
    }
    let ms = crate::models::settings(v).unwrap_or_default();
    let regimen = ms
        .year_settings(year)
        .0
        .corporate_regime
        .unwrap_or(ms.corporate_regime);
    let (_, tipo) = taxpain_models::m200::gross_tax(Money::from_euros(1, 0), regimen, year);
    let tipo = match regimen {
        // El nominal es el del segundo tramo: el del primero sale de la cuota de 100 € de base
        // (sus céntimos son el tipo en centésimas de punto).
        TaxRegime::Micro => {
            let cuota = taxpain_models::m200::gross_tax(Money::from_euros(100, 0), regimen, year).0;
            Rate::from_bp(i32::try_from(cuota.cents()).unwrap_or(2500))
        }
        _ => tipo,
    };
    u8::try_from(tipo.bp() / 100).unwrap_or(25)
}

fn doble_imposicion(ops_year: &[&Op], vs: &[Valor], fisica: bool, tipo_pct: u8) -> DobleImposicion {
    let pais_de = |o: &Op| pais_de(o, vs);
    let mut rentas: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    for o in ops_year {
        match o.kind {
            TipoOp::Dividendo => {
                rentas.entry(pais_de(o)).or_default().0 += o.eur;
            }
            TipoOp::Retencion if pais_de(o) != "ES" => {
                rentas.entry(pais_de(o)).or_default().1 += o.eur;
            }
            _ => {}
        }
    }
    let mut paises = Vec::new();
    for (pais, (r, ret)) in rentas {
        if ret.is_zero() || pais == "ES" {
            continue;
        }
        let conv = CONVENIOS.iter().find(|(p, _, _)| *p == pais);
        let tope = conv.map(|(_, pct, _)| r.apply_rate(Rate::percent(i32::from(*pct))));
        let computable = tope.map_or(ret, |t| ret.min(t));
        let cuota = r.apply_rate(Rate::percent(i32::from(tipo_pct)));
        let deducible = computable.min(cuota).max(Money::ZERO);
        paises.push(RentaPais {
            pais: if pais.is_empty() {
                "sin país".into()
            } else {
                pais
            },
            rentas: r,
            retenido: ret,
            convenio_pct: conv.map(|(_, p, _)| *p),
            convenio_comprobado: conv.is_some_and(|(_, _, ok)| *ok),
            tope_convenio: tope,
            cuota_espanola: cuota,
            deducible,
            exceso: tope.map_or(Money::ZERO, |t| (ret - t).max(Money::ZERO)),
        });
    }
    let retenido = paises.iter().map(|p| p.retenido).sum();
    let deducible = paises.iter().map(|p| p.deducible).sum();
    let nota = if paises.is_empty() {
        "No hay retenciones en origen en el ejercicio.".to_owned()
    } else if fisica {
        format!(
            "Deducción por doble imposición internacional (art. 80 LIRPF): la menor de lo \
             retenido fuera (con el límite del convenio) y lo que pagarían esas rentas en \
             España; aquí, con el {tipo_pct} % del primer tramo del ahorro (el límite real es \
             el tipo medio). Estimación informativa: la calcula Renta Web."
        )
    } else {
        format!(
            "Deducción por doble imposición jurídica internacional (art. 31 LIS), por países: la \
             menor de lo retenido fuera (sin pasar del tipo del convenio) y la cuota española de \
             esas rentas (aquí, al {tipo_pct} %, el tipo de la sociedad en «Datos de los \
             modelos»; en las de menos de 1 M€ de cifra de negocios, el del primer tramo, y si la \
             base pasa de 50.000 € el límite puede ser algo mayor). Los dividendos van íntegros a la base (art. 31.2 LIS) y lo no deducido por \
             falta de cuota se aplica en los años siguientes (31.6). Lo retenido de más sobre el \
             convenio no es gasto deducible: pide su devolución al otro país. Es informativo: \
             la deducción se aplica en el 200."
        )
    };
    DobleImposicion {
        tipo_pct,
        paises,
        retenido,
        deducible,
        nota,
    }
}

/// Informe del ejercicio `year`.
pub fn report(v: &Vault, year: i32) -> Result<Report> {
    let s = settings(v)?;
    let fisica = crate::cripto::es_persona_fisica(v);
    let metodo = if fisica { Metodo::Fifo } else { Metodo::Pmp };
    let al_coste = fisica || s.cartera == Cartera::Coste;
    let (inicio, fecha) = ejercicio(v, year);
    let cs = cuentas(v)?;
    let vs = valores(v)?;
    let todas = ops(v, None)?;
    let mut movs: Vec<Mov> = todas.iter().filter_map(movimiento).collect();
    let mut avisos = Vec::new();
    // Negociación: cada cierre anterior deja los títulos a su valor razonable (el ajuste ya se
    // llevó a resultados), así que el ajuste de este cierre y las ventas parten de ahí.
    if !fisica && s.cartera == Cartera::Negociacion {
        avisos.extend(valorar_cierres_anteriores(
            v, &mut movs, metodo, al_coste, year, inicio, &vs, &cs,
        )?);
    }
    let mut notas = Vec::new();

    let sin_euros: Vec<&Op> = todas
        .iter()
        .filter(|o| o.date >= inicio && o.date <= fecha)
        .filter(|o| o.eur.is_zero() && !o.amount.is_zero())
        .collect();
    let sin_euros_n = sin_euros.len();
    if !sin_euros.is_empty() {
        avisos.push(format!(
            "{} operaciones del ejercicio en otra divisa sin importe en euros: no cuentan hasta \
             que tengan su tipo de cambio. Pulsa «Pasar a euros con el BCE» (descarga los tipos \
             de referencia del BCE de esas fechas) o edítalas con el suyo.",
            sin_euros.len()
        ));
    }

    let (posiciones, calc_avisos) = posiciones_a(v, &movs, metodo, al_coste, fecha, &vs, &cs)?;
    avisos.extend(calc_avisos);
    let nombre = |id: i64| nombre_valor(&vs, id);
    let calc = calculo::calcular(&movs, metodo, al_coste, Some(fecha), &nombre);
    let ops_year: Vec<&Op> = todas
        .iter()
        .filter(|o| o.date >= inicio && o.date <= fecha)
        .collect();

    // Resultado de las ventas del ejercicio.
    let realizados: Vec<RealizadoView> = calc
        .realizados
        .iter()
        .filter(|r| r.fecha >= inicio && r.fecha <= fecha)
        .map(|r| {
            let x = vs.iter().find(|x| x.id == r.valor);
            let derivado = x.is_some_and(|x| x.kind == Clase::Derivado);
            let dos_meses = fisica
                && !derivado
                && r.resultado.is_negative()
                && todas.iter().any(|o| {
                    o.kind == TipoOp::Compra
                        && o.security_id == Some(r.valor)
                        && o.id != r.op
                        && (o.date - r.fecha).whole_days().abs() <= 61
                });
            RealizadoView {
                op: r.op,
                fecha: r.fecha,
                security_id: r.valor,
                ticker: x.map(|x| x.ticker.clone()).unwrap_or_default(),
                name: x.map(|x| x.name.clone()).unwrap_or_default(),
                account: cs
                    .iter()
                    .find(|c| c.id == r.cuenta)
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                cantidad: r.cantidad,
                transmision: r.transmision,
                coste: r.coste,
                resultado: r.resultado,
                dos_meses,
                derivado,
            }
        })
        .collect();
    if realizados.iter().any(|r| r.dos_meses) {
        avisos.push(
            "Hay pérdidas con compras del mismo valor en los dos meses anteriores o posteriores: \
             no se computan hasta que se vendan esos títulos (art. 33.5.f LIRPF)."
                .into(),
        );
    }
    let mut r = Resultado {
        ventas: realizados.iter().filter(|x| !x.derivado).count(),
        ventas_derivados: realizados.iter().filter(|x| x.derivado).count(),
        ..Resultado::default()
    };
    for x in &realizados {
        if x.derivado {
            r.derivados += x.resultado;
        } else if x.resultado.is_negative() {
            r.perdidas += -x.resultado;
        } else {
            r.ganancias += x.resultado;
        }
    }
    r.neto = r.ganancias - r.perdidas;
    for o in &ops_year {
        match o.kind {
            TipoOp::Dividendo => r.dividendos += o.eur,
            TipoOp::Retencion if pais_de(o, &vs) == "ES" => {
                r.retenciones_espana += o.eur;
            }
            TipoOp::Retencion => r.retenciones_extranjero += o.eur,
            TipoOp::Interes => r.intereses += o.eur,
            TipoOp::Comision => r.comisiones += o.eur,
            TipoOp::Impuesto => r.impuestos += o.eur,
            _ => {}
        }
    }
    r.comisiones += calc
        .gastos
        .iter()
        .filter(|g| g.fecha >= inicio && g.fecha <= fecha)
        .map(|g| g.importe)
        .sum::<Money>();

    // Precios y cambios que faltan.
    let sin_precio: Vec<String> = posiciones
        .iter()
        .filter(|p| p.precio.is_none())
        .map(|p| p.ticker.clone())
        .collect();
    if !sin_precio.is_empty() {
        avisos.push(format!(
            "Sin precio de cierre a {fecha} de {}: añádelo en «Precios de cierre» (o sincroniza \
             o importa el extracto que lo trae).",
            sin_precio.join(", ")
        ));
    }
    let sin_cambio: Vec<String> = posiciones
        .iter()
        .filter(|p| p.precio.is_some() && p.cambio.is_none())
        .map(|p| p.divisa.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !sin_cambio.is_empty() {
        avisos.push(format!(
            "Falta el tipo de cambio a {fecha} de {}: pulsa «Tipo del BCE» en «Precios de \
             cierre».",
            sin_cambio.join(", ")
        ));
    }

    if fecha > time::OffsetDateTime::now_utc().date() {
        notas.push(format!(
            "El ejercicio aún no ha terminado: las posiciones a {fecha} se valoran con el último \
             precio y tipo de cambio conocidos (la fecha va junto a cada uno). Al cierre, pide el \
             tipo del BCE del 31 de diciembre."
        ));
    }

    // 720 a 31 de diciembre (aunque el ejercicio no sea natural).
    let dic31 = Date::from_calendar_date(year, Month::December, 31).unwrap_or(fecha);
    let pos_720 = if dic31 == fecha {
        posiciones.clone()
    } else {
        posiciones_a(v, &movs, metodo, al_coste, dic31, &vs, &cs)?.0
    };
    let m720 = m720_de(year, dic31, &pos_720, fisica);
    let doble = doble_imposicion(&ops_year, &vs, fisica, tipo_cuota(v, year, fisica));

    // Notas de criterio.
    if fisica {
        notas.push(
            "Persona física: las ventas se calculan con FIFO (art. 37.2 LIRPF) y son ganancias o \
             pérdidas patrimoniales de la base del ahorro (arts. 33, 46 y 49 LIRPF); los \
             dividendos e intereses, rendimientos del capital mobiliario (art. 25 LIRPF)."
                .into(),
        );
    } else {
        notas.push(match s.cartera {
            Cartera::Coste => "Clasificación: activos financieros a coste (PGC PYMES, NRV 9.ª \
                2.3) o, en el PGC normal, a valor razonable con cambios en el patrimonio neto: \
                comisiones al coste y, al vender, coste medio ponderado por valor (NRV 9.ª). \
                Ojo: en el PGC normal las acciones y ETF cotizados que no son de negociación no \
                pueden ir a coste (NRV 9.ª 2.4.b): van a valor razonable con cambios en \
                resultados salvo que se elija el patrimonio neto. Si la sociedad compra para \
                vender a corto plazo (cartera de negociación), cámbialo en los ajustes: \
                entonces van a valor razonable con cambios en resultados (7630/6630 en el PGC; \
                763/663 en el PYMES) y las comisiones, a gastos."
                .to_owned(),
            Cartera::Negociacion => "Clasificación: mantenidos para negociar (NRV 9.ª 2.2 \
                PYMES / 2.1 PGC): a valor razonable con cambios en la cuenta de pérdidas y \
                ganancias y las comisiones a gastos. El coste de las posiciones es su valor en \
                libros (el valor razonable del cierre anterior más lo comprado después) y la \
                plusvalía latente al cierre es el ajuste a valor razonable (7630/6630; 763/663 en el PYMES), que entra en la base del 200 \
                al ir a resultados: se propone, no se aplica sin tu confirmación."
                .to_owned(),
        });
        notas.push(
            "Los dividendos de participaciones de menos del 5 % no tienen la exención del art. \
             21 LIS: van íntegros a la base; la retención en origen se recupera con la deducción \
             del art. 31 LIS (abajo) y la española (473) se resta en el 200."
                .into(),
        );
    }
    if r.ventas_derivados > 0 {
        notas.push(if fisica {
            "Opciones: la prima pagada es el coste y lo cobrado al cerrarla (cero si vence sin \
             valor) el valor de transmisión; la diferencia es una ganancia o pérdida \
             patrimonial de la base del ahorro (art. 33 LIRPF, `TODO(VERIFY)` con la doctrina \
             de la DGT sobre opciones). Va en la línea «Derivados», aparte de las ventas."
                .to_owned()
        } else {
            format!(
                "Opciones y otros derivados: activos financieros mantenidos para negociar (NRV \
                 9.ª): la prima pagada se activa ({}) y, al cerrarse o vencer, la diferencia \
                 con lo cobrado va a resultados ({} / {}). Es resultado contable y entra en \
                 la base del 200. Va en la línea «Derivados», aparte de las ventas de valores.",
                super::asientos::DERIVADOS,
                s.cuentas.beneficio_vr,
                s.cuentas.perdida_vr
            )
        });
    }
    notas.push(
        "Divisas: cada operación se pasa a euros con el tipo del día que da el bróker (en IBKR, \
         «fxRateToBase») o, si no, con el tipo de referencia del BCE (NRV 11.ª: tipo de contado \
         de la fecha de la operación)."
            .into(),
    );

    let asientos = if fisica {
        Vec::new()
    } else {
        super::asientos::proponer(&s, &cs, &vs, &ops_year, &calc, &posiciones, fecha)
    };
    let ops_ejercicio = ops_year.len();
    let cuentas_view = cs
        .iter()
        .map(|c| CuentaView {
            ops: todas
                .iter()
                .filter(|o| o.account_id == c.id || o.to_account_id == Some(c.id))
                .count(),
            conector: match c.broker {
                super::Broker::Ibkr if c.token => "api",
                super::Broker::Ibkr => "fichero",
                super::Broker::Xtb => "pdf",
                super::Broker::Otro => "manual",
            },
            cuenta: c.clone(),
        })
        .collect();
    let coste_total = posiciones.iter().map(|p| p.coste).sum();
    let valor_total: Money = posiciones.iter().filter_map(|p| p.valor).sum();
    let latente_total = posiciones.iter().filter_map(|p| p.latente).sum();
    Ok(Report {
        year,
        inicio,
        fecha,
        persona_fisica: fisica,
        metodo,
        settings: s,
        cuentas: cuentas_view,
        posiciones,
        coste_total,
        valor_total,
        latente_total,
        resultado: r,
        realizados,
        doble_imposicion: doble,
        m720,
        asientos,
        ops_ejercicio,
        sin_euros: sin_euros_n,
        avisos,
        notas,
    })
}
