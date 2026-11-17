//! Cruces de la «segunda opinión» sobre lo que hizo la gestoría: unos modelos frente a otros
//! (390 y 303, resúmenes anuales y trimestrales, 200 y 202), los resúmenes anuales por NIF
//! frente a las facturas (347, 349, 190, 180), los plazos (presentaciones fuera de plazo con su
//! recargo, depósito de cuentas y legalización de libros), los pagos del banco frente a los
//! modelos, los socios y administradores (escrituras, Nóminas y saldos de la 551/552) y las
//! retenciones del diario (la 4751) frente a los 111 y el 190.
//!
//! Cada aviso dice qué no cuadra, por cuánto y qué hacer. Como el resto de la revisión, sólo
//! lee y es orientativo.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::calendar::{
    Deadline, Periodicity, Persona, TaxpayerProfile, calendar_for, deadlines_in_year,
};
use taxpain_core::money::Money;
use time::Date;

use super::Salida;
use super::expediente::{Expediente, nif_clave};
use super::modelos::{Clave, Declarado, declarados};

mod anuales;
pub(crate) mod datos;
mod devoluciones;
pub(crate) mod pagos;
pub(crate) mod plazos;
mod retenciones;
pub(crate) mod socios;
mod terceros;

/// Lo que comparten los cruces.
pub(crate) struct Contexto<'a> {
    pub exp: &'a Expediente,
    pub hoy: Date,
    /// Modelos presentados de los ejercicios que aún se pueden revisar.
    pub dec: BTreeMap<Clave, Declarado>,
    /// Plazos por (modelo, ejercicio, periodo).
    plazos: BTreeMap<Clave, Deadline>,
    /// Años con los festivos oficiales cargados en TaxPain: en los demás, los plazos se calculan
    /// sólo con los fines de semana y pueden acabar unos días más tarde de lo calculado.
    festivos: BTreeSet<i32>,
}

impl<'a> Contexto<'a> {
    fn new(exp: &'a Expediente, hoy: Date) -> Self {
        let desde = super::revisable_desde(hoy);
        let dec: BTreeMap<Clave, Declarado> = declarados(exp)
            .into_iter()
            .filter(|((_, y, _), _)| *y >= desde)
            .collect();
        let perfil = perfil_amplio(exp);
        let mut plazos = BTreeMap::new();
        let mut festivos = BTreeSet::new();
        for año in desde..=hoy.year() + 2 {
            let (h, cargados) = calendar_for(año, perfil.ccaa, &perfil.festivos_locales);
            if cargados {
                festivos.insert(año);
            }
            for d in deadlines_in_year(&perfil, año, &h) {
                plazos.insert(
                    (d.model.code().to_owned(), d.fiscal_year, d.period.code()),
                    d,
                );
            }
        }
        Contexto {
            exp,
            hoy,
            dec,
            plazos,
            festivos,
        }
    }

    /// ¿Tiene TaxPain los festivos oficiales del año en que acaba el plazo?
    pub fn con_festivos(&self, plazo: &Deadline) -> bool {
        self.festivos.contains(&plazo.due_on.year())
    }

    /// Plazo de un modelo («303», 2025, «1T»), o de un trámite («deposito-cuentas», 2024, «0A»).
    pub fn plazo(&self, modelo: &str, ejercicio: i32, periodo: &str) -> Option<&Deadline> {
        self.plazos
            .get(&(modelo.to_owned(), ejercicio, periodo.to_owned()))
    }

    /// Un modelo presentado.
    pub fn modelo(&self, modelo: &str, ejercicio: i32, periodo: &str) -> Option<&Declarado> {
        self.dec
            .get(&(modelo.to_owned(), ejercicio, periodo.to_owned()))
    }

    /// Modelos presentados de un tipo, en orden.
    pub fn de(&self, modelo: &'a str) -> impl Iterator<Item = (&Clave, &Declarado)> + '_ {
        self.dec.iter().filter(move |((m, _, _), _)| m == modelo)
    }

    /// ¿Es una sociedad (o la empresa no es una persona física)?
    pub fn sociedad(&self) -> bool {
        sociedad(self.exp)
    }
}

/// ¿Es una sociedad la empresa? Por el perfil fiscal o, sin él, por el NIF.
pub(crate) fn sociedad(exp: &Expediente) -> bool {
    match &exp.perfil {
        Some(p) => p.persona == Persona::Juridica,
        None => exp.own_nif.as_deref().is_some_and(es_sociedad),
    }
}

/// ¿Es de una sociedad este NIF? (empieza por A, B…; no por cifra, X, Y, Z, K, L o M).
fn es_sociedad(nif: &str) -> bool {
    nif_clave(nif)
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() && !"XYZKLM".contains(c))
}

/// El perfil fiscal con todas las obligaciones que se pueden tener, para saber el plazo de
/// cualquier modelo que esté en la carpeta (aunque el perfil no lo marque). Sin perfil, el de
/// una SL o el de un autónomo según el NIF.
pub(crate) fn perfil_amplio(exp: &Expediente) -> TaxpayerProfile {
    let mut p = exp.perfil.clone().unwrap_or_else(|| {
        if exp.own_nif.as_deref().is_some_and(es_sociedad) {
            TaxpayerProfile::sociedad_limitada()
        } else {
            TaxpayerProfile::autonomo()
        }
    });
    p.retiene_trabajo_profesionales = true;
    p.retiene_alquileres = true;
    p.retiene_capital_mobiliario = true;
    p.obligado_347 = true;
    if p.intracomunitarias.is_none() {
        p.intracomunitarias = Some(Periodicity::Trimestral);
    }
    if p.persona == Persona::Juridica {
        p.pagos_fraccionados_is = true;
    }
    p
}

/// Casillas del resultado de un modelo, la primera que esté: [71] en el 303, [30] en el 111,
/// [05] en el 115, [14] en el 123, [19] en el 130, [03] (o [34]) en el 202 y [01586] (o [00621]) en el
/// 200 (en una rectificativa del 200, sólo la [00621]: ver [`a_ingresar`]). Vacío si el modelo
/// no tiene resultado (los informativos).
fn casillas_resultado(modelo: &str) -> &'static [u32] {
    match modelo {
        "303" => &[71],
        "111" => &[30, 28],
        "115" => &[5, 3],
        "123" => &[14, 9],
        "130" => &[19],
        // [03] por la cuota (art. 40.2 LIS) o [34] por la base (art. 40.3).
        "202" => &[3, 34],
        "200" => &[1586, 621],
        _ => &[],
    }
}

/// ¿Tiene el modelo una casilla de resultado a ingresar?
pub(crate) fn tiene_resultado(modelo: &str) -> bool {
    !casillas_resultado(modelo).is_empty()
}

/// Casilla [00621] del 200: «Resultado» de la autoliquidación rectificativa (o de la
/// complementaria), lo que sale a ingresar o a devolver con ella.
const RESULTADO_RECTIFICATIVA_200: u32 = 621;
/// Casilla [00866] del 200: «Rectificación», lo que se pide devolver con la rectificativa.
const RECTIFICACION_200: u32 = 866;

/// ¿Es un 200 que rectifica otro (lo dice su tipo o su justificante anterior, o lleva la
/// [00866])? Su [01586] es el resultado de la liquidación entera, no lo que sale a ingresar o a
/// devolver con ella: eso es la [00621].
pub(crate) fn rectificativa_200(modelo: &str, d: &Declarado) -> bool {
    modelo == "200" && (d.rectifica || !d.c(RECTIFICACION_200).is_zero())
}

/// Las casillas del resultado de esta declaración: las del modelo ([`casillas_resultado`]) o, en
/// una rectificativa del 200, la [00621].
fn casillas_resultado_de(modelo: &str, d: &Declarado) -> &'static [u32] {
    if rectificativa_200(modelo, d) {
        &[RESULTADO_RECTIFICATIVA_200]
    } else {
        casillas_resultado(modelo)
    }
}

/// ¿Se sabe lo que salía a ingresar? No si no se han podido leer las casillas de la declaración
/// o las del resultado.
pub(crate) fn resultado_sabido(modelo: &str, d: &Declarado) -> bool {
    d.con_casillas()
        && !casillas_resultado_de(modelo, d)
            .iter()
            .any(|c| d.dudosas.contains(c))
}

/// Lo que sale a ingresar de un modelo (positivo) o a compensar o devolver (negativo), con la
/// casilla de su resultado ([`casillas_resultado`]). `None` si el modelo no tiene resultado, si
/// no se leyó la casilla o si no se sabe ([`resultado_sabido`]). En una rectificativa del 200,
/// la [00621]; vacía, cero: la rectificativa no pide ni ingresa nada.
pub(crate) fn a_ingresar(modelo: &str, d: &Declarado) -> Option<Money> {
    if !resultado_sabido(modelo, d) {
        return None;
    }
    if rectificativa_200(modelo, d) {
        return Some(d.c(RESULTADO_RECTIFICATIVA_200));
    }
    casillas_resultado(modelo)
        .iter()
        .find_map(|c| d.casillas.get(c).copied())
}

/// El pago que se ve en el banco por una declaración: lo que sale a ingresar con ella
/// ([`a_ingresar`]) y desde cuándo se puede pagar (su presentación). En una rectificativa del 200
/// que no ingresa nada (sale a devolver o a cero), el pago es el de la original, con su fecha:
/// sin esto, el cargo de la original quedaría como un cargo de Hacienda sin modelo.
pub(crate) fn a_pagar(modelo: &str, d: &Declarado) -> Option<(Money, Option<Date>)> {
    let propio = a_ingresar(modelo, d).filter(|c| *c > Money::ZERO);
    if propio.is_some() || !rectificativa_200(modelo, d) || d.casillas_original.is_empty() {
        return propio.map(|c| (c, d.presentado));
    }
    let original = Declarado {
        casillas: d.casillas_original.clone(),
        presentado: d.original,
        ..Declarado::default()
    };
    a_ingresar(modelo, &original)
        .filter(|c| *c > Money::ZERO)
        .map(|c| (c, d.original))
}

/// «el 303 del 1T 2025», «el 390 de 2025».
pub(crate) fn nombre(modelo: &str, ejercicio: i32, periodo: &str) -> String {
    if periodo == "0A" || periodo.is_empty() {
        format!("el {modelo} de {ejercicio}")
    } else {
        format!("el {modelo} del {periodo} {ejercicio}")
    }
}

/// «el 303…» → «El 303…».
pub(crate) fn primera(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|p| p.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// «Escritura de constitución (en TaxPain)» → «escritura de constitución (en TaxPain)»: sólo
/// la primera letra, para no estropear los nombres que van dentro.
pub(crate) fn minuscula(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|p| p.to_lowercase().chain(c).collect())
        .unwrap_or_default()
}

pub(crate) fn revisar(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let cx = Contexto::new(exp, hoy);
    anuales::revisar(&cx, s);
    terceros::revisar(&cx, s);
    plazos::revisar(&cx, s);
    pagos::revisar(&cx, s);
    devoluciones::revisar(&cx, s);
    socios::revisar(&cx, s);
    retenciones::revisar(&cx, s);
}

/// Los cruces que no necesitan la carpeta: unos modelos frente a otros, lo presentado fuera de
/// plazo y las retenciones del diario frente a los 111 y el 190. Los de los terceros (frente a
/// las facturas), los pagos (frente a los extractos), los socios (frente a las escrituras) y el
/// Registro Mercantil (sus justificantes) sin carpeta darían por falta lo que sólo no está en
/// ella.
pub(crate) fn revisar_sin_carpeta(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let cx = Contexto::new(exp, hoy);
    anuales::revisar(&cx, s);
    plazos::revisar_sin_carpeta(&cx, s);
    retenciones::revisar(&cx, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doscientos(rectifica: bool, casillas: &[(u32, i64)]) -> Declarado {
        Declarado {
            casillas: casillas
                .iter()
                .map(|(k, v)| (*k, Money::from_cents(*v)))
                .collect(),
            rectifica,
            ..Declarado::default()
        }
    }

    /// En la autoliquidación rectificativa del 200, lo que sale a devolver es la [00621] (la
    /// [00866] lleva lo mismo en positivo); la [01586] es el resultado de toda la liquidación. Si
    /// la [00621] está vacía, la rectificativa no pide nada.
    #[test]
    fn el_resultado_de_la_rectificativa_del_200_es_la_00621() {
        let m = Money::from_cents;
        let original = doscientos(false, &[(1586, 500_000)]);
        assert_eq!(a_ingresar("200", &original), Some(m(500_000)));
        let rectificativa = doscientos(true, &[(1586, 450_000), (621, -50_000), (866, 50_000)]);
        assert_eq!(a_ingresar("200", &rectificativa), Some(m(-50_000)));
        // Sin el tipo, la [00866] dice que lo es.
        let sin_tipo = doscientos(false, &[(1586, 450_000), (621, -50_000), (866, 50_000)]);
        assert_eq!(a_ingresar("200", &sin_tipo), Some(m(-50_000)));
        // La que sólo cambia la base negativa: nada que ingresar ni devolver.
        let sin_dinero = doscientos(true, &[(1586, -120_000)]);
        assert_eq!(a_ingresar("200", &sin_dinero), Some(Money::ZERO));
        // Con la [00621] dudosa, no se sabe.
        let mut dudosa = doscientos(true, &[(1586, 450_000)]);
        dudosa.dudosas.insert(621);
        assert_eq!(a_ingresar("200", &dudosa), None);
    }
}
