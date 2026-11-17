//! Ventanilla única (OSS, «régimen de la Unión»): ventas a distancia intracomunitarias de bienes
//! y servicios de telecomunicaciones, de radiodifusión y televisión y electrónicos (TBE) a
//! consumidores finales de otros Estados miembros.
//!
//! - Lugar de realización (arts. 68.Tres.a y 70.Uno.4.º y 8.º LIVA): el Estado miembro del
//!   consumidor, salvo que el proveedor, establecido en un solo Estado miembro, no supere
//!   **10.000 €** (sin IVA) de esas operaciones en el año anterior ni en el año en curso (art. 73
//!   LIVA). Superado en el año en curso, tributa en destino **la operación con la que se supera**
//!   y las siguientes. Se puede optar por tributar en destino aunque no se supere (art. 73,
//!   último párrafo: la opción vale al menos dos años).
//! - El IVA de destino se declara en España en el modelo 369 trimestral (arts. 163 unvicies a
//!   163 tervicies LIVA; Orden HAC/610/2021) y no en el 303: allí la base va en la casilla
//!   informativa [123] y en el 390 en la [126] (diseños de registro DR303e26v101 y dr390e2026).
//!   El alta en el régimen se comunica con el modelo 035.
//!
//! En la factura, las líneas van como **no sujetas por reglas de localización** (N2) desde el
//! punto de vista del IVA español, y el IVA del Estado miembro de consumo se suma aparte
//! ([`Totals::oss`]): se cobra al cliente (total y a cobrar) y se contabiliza en una subcuenta
//! propia de la 477 por país ([`cuenta_oss`]), que la liquidación del 303 no salda.
//!
//! La tabla de tipos generales ([`TIPOS_GENERALES`]) es un dato con fecha y fuente que hay que
//! mantener: la app pide al usuario que confirme el tipo de cada país antes de usarlo.

use serde::{Deserialize, Serialize};
use time::{Date, Month};

use super::model::{NotSubject, VatTreatment};
use super::totals::Totals;
use crate::accounting::account::{AccountCode, acc};
use crate::money::{Money, Rate};

/// Umbral anual común de ventas a distancia y servicios TBE a consumidores de otros Estados
/// miembros (art. 73 LIVA; art. 59 quater de la Directiva 2006/112/CE): 10.000 €, sin IVA.
pub const UMBRAL: Money = Money::from_euros(10_000, 0);

/// Clase de operación del régimen de la Unión (las dos que cuentan para el umbral del art. 73).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OssSupply {
    /// Servicios de telecomunicaciones, radiodifusión y televisión o electrónicos a
    /// consumidores finales (art. 70.Uno.4.º y 8.º LIVA).
    Servicios,
    /// Ventas a distancia intracomunitarias de bienes (art. 68.Tres.a LIVA).
    Bienes,
}

impl OssSupply {
    pub fn label(self) -> &'static str {
        match self {
            OssSupply::Servicios => "servicios TBE",
            OssSupply::Bienes => "ventas a distancia de bienes",
        }
    }
}

/// IVA del Estado miembro de consumo de una factura (en euros).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OssVat {
    /// Estado miembro de consumo (ISO, «FR»; Grecia «EL»).
    pub country: String,
    pub supply: OssSupply,
    /// Tipo de IVA de ese Estado.
    pub rate: Rate,
    /// Base: las líneas no sujetas por reglas de localización (N2) de la factura.
    pub base: Money,
    /// Cuota del IVA de destino (base × tipo, al céntimo).
    pub vat: Money,
}

/// Suma a `totals` el IVA de destino: base = líneas N2, cuota = base × `rate`. El total y lo
/// que se cobra lo incluyen; la cuota española (`vat`, `tax_total`) no.
pub fn apply(totals: &mut Totals, country: &str, supply: OssSupply, rate: Rate) {
    let base: Money = totals
        .breakdown
        .iter()
        .filter(|b| b.vat == VatTreatment::NotSubject(NotSubject::N2))
        .map(|b| b.base)
        .sum();
    let vat = base.apply_rate(rate);
    totals.invoice_total += vat;
    totals.payable += vat;
    totals.oss = Some(OssVat {
        country: country.trim().to_ascii_uppercase(),
        supply,
        rate,
        base,
        vat,
    });
}

/// Tipo general de IVA de un Estado miembro: (código, nombre, tipo en centésimas de punto).
pub type TipoPais = (&'static str, &'static str, i32);

/// Fecha a la que se refiere la tabla de [`TIPOS_GENERALES`].
pub const TIPOS_FECHA: &str = "2025-08-01";

/// Fuente de la tabla de [`TIPOS_GENERALES`].
pub const TIPOS_FUENTE: &str = "Comisión Europea, «VAT rates applied in the Member States of the \
     European Union» y base de datos TEDB (taxation-customs.ec.europa.eu)";

/// Tipos generales (estándar) de IVA de los Estados miembros distintos de España, a
/// [`TIPOS_FECHA`]. Recogen los cambios conocidos hasta esa fecha: Finlandia 25,5 % desde el
/// 1/9/2024, Eslovaquia 23 % desde el 1/1/2025, Estonia 24 % desde el 1/7/2025 y Rumanía 21 %
/// desde el 1/8/2025. Irlanda del Norte (XI) sólo cuenta como Estado miembro para los bienes,
/// con el tipo del Reino Unido.
/// `TODO(VERIFY)`: cotejar con la fuente oficial vigente; los tipos reducidos (libros
/// electrónicos, prensa…) no están: el usuario pone el tipo en la factura y confirma el de cada
/// país antes de usarlo.
pub const TIPOS_GENERALES: &[TipoPais] = &[
    ("AT", "Austria", 2000),
    ("BE", "Bélgica", 2100),
    ("BG", "Bulgaria", 2000),
    ("CY", "Chipre", 1900),
    ("CZ", "Chequia", 2100),
    ("DE", "Alemania", 1900),
    ("DK", "Dinamarca", 2500),
    ("EE", "Estonia", 2400),
    ("EL", "Grecia", 2400),
    ("FI", "Finlandia", 2550),
    ("FR", "Francia", 2000),
    ("HR", "Croacia", 2500),
    ("HU", "Hungría", 2700),
    ("IE", "Irlanda", 2300),
    ("IT", "Italia", 2200),
    ("LT", "Lituania", 2100),
    ("LU", "Luxemburgo", 1700),
    ("LV", "Letonia", 2100),
    ("MT", "Malta", 1800),
    ("NL", "Países Bajos", 2100),
    ("PL", "Polonia", 2300),
    ("PT", "Portugal", 2300),
    ("RO", "Rumanía", 2100),
    ("SE", "Suecia", 2500),
    ("SI", "Eslovenia", 2200),
    ("SK", "Eslovaquia", 2300),
    ("XI", "Irlanda del Norte", 2000),
];

/// Código del país como en la tabla («GR» → «EL»).
pub fn codigo(country: &str) -> String {
    crate::eu::vies_prefix(country)
}

fn fila(country: &str) -> Option<(usize, &'static TipoPais)> {
    let c = codigo(country);
    TIPOS_GENERALES.iter().enumerate().find(|(_, t)| t.0 == c)
}

/// Nombre en español del Estado miembro (o el código si no está en la tabla).
pub fn nombre_pais(country: &str) -> String {
    fila(country).map_or_else(|| codigo(country), |(_, t)| t.1.to_owned())
}

/// Tipo general de la tabla para `country`.
pub fn tipo_general(country: &str) -> Option<Rate> {
    fila(country).map(|(_, t)| Rate::from_bp(t.2))
}

/// Subcuenta de la 477 para el IVA de destino de `country`: «4779» + el número de orden del país
/// en [`TIPOS_GENERALES`] (Austria 1, Bélgica 2… Francia 11, Alemania 6), con los dígitos de las
/// subcuentas de la empresa (4779011 con 7). Si no cabe, la 4779.
pub fn cuenta_oss(country: &str, width: usize) -> AccountCode {
    let n = fila(country).map_or(99, |(i, _)| i + 1);
    acc("4779")
        .subaccount(width.max(5), u32::try_from(n).unwrap_or(99))
        .unwrap_or_else(|_| acc("4779"))
}

/// Raíz de las subcuentas del IVA de destino.
pub const CUENTA_OSS_RAIZ: &str = "4779";

/// Situación del umbral del art. 73 LIVA en un año.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Umbral {
    pub year: i32,
    /// Ventas a distancia y servicios TBE a consumidores de otros Estados miembros del año
    /// anterior (sin IVA).
    pub anterior: Money,
    /// Las del año en curso hasta la última operación.
    pub en_curso: Money,
    /// El año anterior pasó de 10.000 €: todo el año tributa en destino.
    pub superado_anterior: bool,
    /// Fecha de la operación con la que el año en curso pasa de 10.000 €.
    pub superado_el: Option<Date>,
    /// Desde cuándo tributan en destino (1 de enero si se superó el año anterior).
    pub destino_desde: Option<Date>,
}

/// Umbral de `year` con lo del año anterior y las operaciones del año (fecha y base, en
/// cualquier orden). La que hace pasar el acumulado de 10.000 € ya tributa en destino.
pub fn umbral(year: i32, anterior: Money, ventas: &[(Date, Money)]) -> Umbral {
    let mut v: Vec<(Date, Money)> = ventas.to_vec();
    v.sort_by_key(|x| x.0);
    let mut acumulado = Money::ZERO;
    let mut superado_el = None;
    for (d, base) in &v {
        acumulado += *base;
        if superado_el.is_none() && acumulado > UMBRAL {
            superado_el = Some(*d);
        }
    }
    let superado_anterior = anterior > UMBRAL;
    let destino_desde = if superado_anterior {
        Date::from_calendar_date(year, Month::January, 1).ok()
    } else {
        superado_el
    };
    Umbral {
        year,
        anterior,
        en_curso: acumulado,
        superado_anterior,
        superado_el,
        destino_desde,
    }
}

/// Trimestre (1-4) de una fecha.
pub fn trimestre(d: Date) -> u8 {
    (u8::from(d.month()) - 1) / 3 + 1
}

/// Último día para presentar el 369 trimestral: el último día del mes siguiente al trimestre
/// (art. 163 terdecies LIVA; no se traslada si es festivo, ver el calendario).
pub fn vencimiento_369(year: i32, quarter: u8) -> Option<Date> {
    let (y, m) = if quarter >= 4 {
        (year + 1, Month::January)
    } else {
        (year, Month::try_from(quarter * 3 + 1).ok()?)
    };
    let next = if m == Month::December {
        Date::from_calendar_date(y + 1, Month::January, 1).ok()?
    } else {
        Date::from_calendar_date(y, m.next(), 1).ok()?
    };
    next.previous_day()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::{InvoiceLine, RoundingMode, compute_totals};
    use time::macros::date;

    fn n2(cents: i64) -> InvoiceLine {
        InvoiceLine {
            description: "Suscripción".into(),
            quantity_milli: 1000,
            unit_price: Money::from_cents(cents),
            discount: Rate::ZERO,
            vat: VatTreatment::NotSubject(NotSubject::N2),
            surcharge: None,
            withholding: None,
            advance: false,
        }
    }

    #[test]
    fn iva_de_destino_se_suma_al_total_y_no_a_la_cuota_espanola() {
        let mut t = compute_totals(&[n2(10_000)], RoundingMode::PerRate);
        apply(&mut t, "fr", OssSupply::Servicios, Rate::percent(20));
        let o = t.oss.as_ref().unwrap();
        assert_eq!(o.country, "FR");
        assert_eq!(o.base, Money::from_euros(100, 0));
        assert_eq!(o.vat, Money::from_euros(20, 0));
        assert_eq!(t.vat, Money::ZERO);
        assert_eq!(t.tax_total, Money::ZERO);
        assert_eq!(t.invoice_total, Money::from_euros(120, 0));
        assert_eq!(t.payable, Money::from_euros(120, 0));
    }

    #[test]
    fn tabla_de_tipos_y_subcuentas() {
        assert_eq!(tipo_general("FR"), Some(Rate::percent(20)));
        assert_eq!(tipo_general("DE"), Some(Rate::percent(19)));
        assert_eq!(tipo_general("GR"), Some(Rate::percent(24)));
        assert_eq!(tipo_general("FI"), Some(Rate::from_bp(2550)));
        assert_eq!(tipo_general("ES"), None);
        assert_eq!(nombre_pais("DE"), "Alemania");
        assert_eq!(cuenta_oss("FR", 7).as_str(), "4779011");
        assert_eq!(cuenta_oss("DE", 7).as_str(), "4779006");
        assert_eq!(cuenta_oss("DE", 10).as_str(), "4779000006");
        // Todos los Estados miembros salvo España, sin repetir.
        let n = crate::eu::EU_COUNTRIES.len() - 1;
        assert_eq!(TIPOS_GENERALES.len(), n);
        for c in crate::eu::EU_COUNTRIES.iter().filter(|c| **c != "ES") {
            assert!(tipo_general(c).is_some(), "{c}");
        }
    }

    #[test]
    fn umbral_se_supera_con_la_operacion_que_pasa_de_diez_mil() {
        let u = umbral(
            2026,
            Money::from_euros(4_000, 0),
            &[
                (date!(2026 - 05 - 10), Money::from_euros(6_000, 0)),
                (date!(2026 - 02 - 01), Money::from_euros(4_000, 0)),
                (date!(2026 - 09 - 01), Money::from_euros(1, 0)),
            ],
        );
        // 4.000 + 6.000 = 10.000 no supera; el euro de septiembre sí.
        assert_eq!(u.en_curso, Money::from_euros(10_001, 0));
        assert_eq!(u.superado_el, Some(date!(2026 - 09 - 01)));
        assert_eq!(u.destino_desde, Some(date!(2026 - 09 - 01)));
        assert!(!u.superado_anterior);
    }

    #[test]
    fn superado_el_anio_anterior_tributa_en_destino_desde_enero() {
        let u = umbral(2026, Money::from_euros(10_000, 1), &[]);
        assert!(u.superado_anterior);
        assert_eq!(u.destino_desde, Some(date!(2026 - 01 - 01)));
        let u = umbral(2026, Money::from_euros(10_000, 0), &[]);
        assert_eq!(u.destino_desde, None);
    }

    #[test]
    fn vencimientos_del_369() {
        assert_eq!(vencimiento_369(2026, 1), Some(date!(2026 - 04 - 30)));
        assert_eq!(vencimiento_369(2026, 3), Some(date!(2026 - 10 - 31)));
        assert_eq!(vencimiento_369(2026, 4), Some(date!(2027 - 01 - 31)));
        assert_eq!(trimestre(date!(2026 - 11 - 17)), 4);
    }
}
