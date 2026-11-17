//! Facturas en moneda extranjera: tipo de cambio y paso a euros.
//!
//! Normas (contrastadas con el BOE el 24/10/2026):
//! - **Art. 79.Once Ley 37/1992 (LIVA)**: «En las operaciones cuya contraprestación se hubiese
//!   fijado en moneda o divisa distintas de las españolas, se aplicará el tipo de cambio
//!   vendedor, fijado por el Banco de España, que esté vigente en el momento del devengo.»
//! - Los cambios oficiales del Banco de España son los **tipos de referencia del euro que publica
//!   el BCE**: cada día hábil el Banco de España publica en el BOE la resolución «por la que se
//!   publican los cambios del euro correspondientes al día …, publicados por el Banco Central
//!   Europeo, que tendrán la consideración de cambios oficiales, de acuerdo con lo dispuesto en
//!   el artículo 36 de la Ley 46/1998, de 17 de diciembre, sobre la Introducción del Euro». La
//!   Directiva 2006/112/CE (art. 91.2) también admite el último tipo publicado por el BCE.
//! - **Art. 12.1 RD 1619/2012** (Reglamento de facturación): «Los importes que figuran en las
//!   facturas podrán expresarse en cualquier moneda, a condición de que el importe del Impuesto
//!   que, en su caso, se repercuta se exprese en euros, utilizando a tal efecto el tipo de cambio
//!   a que se refiere el artículo 79.Once de la Ley del Impuesto.»
//!
//! Decisiones (D7: nada de coma flotante):
//! - El tipo se guarda como lo publican el BCE y el Banco de España: **unidades de la moneda por
//!   1 €** (1 € = 1,1193 USD), en millonésimas (`i64`, 6 decimales; el BCE publica como mucho 5
//!   cifras significativas).
//! - Euros = importe en la moneda ÷ tipo, redondeado al céntimo *half away from zero* (0,005 →
//!   0,01), el mismo criterio que el resto de importes (`money`).
//! - Se convierte la **base de cada grupo de IVA** (tratamiento + recargo), no cada línea: lo que
//!   el art. 79.Once convierte es la base imponible, y la cuota en euros es esa base en euros por
//!   el tipo (art. 12.1 RD 1619/2012). Así la cuota en euros es exacta sobre la base en euros,
//!   sea cual sea el redondeo de la factura (por tipo o por línea).
//! - Las líneas en euros (contabilidad, anticipos de la 438) son la base de cada línea
//!   convertida; si por redondeo no suman la base del grupo, la diferencia (como mucho medio
//!   céntimo por línea) va a la línea de mayor importe del grupo. Cada línea en euros es
//!   «1 × base»: la cantidad y el precio unitario sólo existen en la moneda de la factura.
//! - Las retenciones en euros salen de esas líneas: su base puede diferir en un céntimo de
//!   convertir directamente la base de la retención.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::Date;

use crate::invoice::totals::{RoundingMode, Totals, compute_totals, line_base};
use crate::invoice::{InvoiceLine, VatTreatment};
use crate::money::{Money, Rate};

/// Moneda admitida en las facturas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrencyInfo {
    /// ISO 4217.
    pub code: &'static str,
    pub name: &'static str,
    /// El BCE publica su tipo de referencia (botón «Tipo del BCE»); si no, se escribe a mano.
    pub ecb: bool,
}

const fn c(code: &'static str, name: &'static str, ecb: bool) -> CurrencyInfo {
    CurrencyInfo { code, name, ecb }
}

/// Monedas con dos decimales (ISO 4217). Las de cero decimales (yen, won, corona islandesa) no
/// se admiten: los importes son céntimos. Lista del BCE contrastada con su API el 24/10/2026
/// (el lev búlgaro dejó de publicarse el 31/12/2025: Bulgaria usa el euro desde 2026).
pub const CURRENCIES: &[CurrencyInfo] = &[
    c("USD", "Dólar estadounidense", true),
    c("GBP", "Libra esterlina", true),
    c("CHF", "Franco suizo", true),
    c("CAD", "Dólar canadiense", true),
    c("AUD", "Dólar australiano", true),
    c("NZD", "Dólar neozelandés", true),
    c("SEK", "Corona sueca", true),
    c("NOK", "Corona noruega", true),
    c("DKK", "Corona danesa", true),
    c("PLN", "Esloti polaco", true),
    c("CZK", "Corona checa", true),
    c("HUF", "Forinto húngaro", true),
    c("RON", "Leu rumano", true),
    c("TRY", "Lira turca", true),
    c("MXN", "Peso mexicano", true),
    c("BRL", "Real brasileño", true),
    c("CNY", "Yuan chino", true),
    c("HKD", "Dólar de Hong Kong", true),
    c("SGD", "Dólar de Singapur", true),
    c("INR", "Rupia india", true),
    c("ILS", "Nuevo séquel israelí", true),
    c("ZAR", "Rand sudafricano", true),
    c("THB", "Bat tailandés", true),
    c("MYR", "Ringit malayo", true),
    c("PHP", "Peso filipino", true),
    c("IDR", "Rupia indonesia", true),
    c("AED", "Dírham de los EAU", false),
    c("SAR", "Riyal saudí", false),
    c("MAD", "Dírham marroquí", false),
    c("COP", "Peso colombiano", false),
    c("PEN", "Sol peruano", false),
];

/// Datos de una moneda admitida (`None` si no se admite o es el euro).
pub fn currency(code: &str) -> Option<&'static CurrencyInfo> {
    CURRENCIES.iter().find(|c| c.code == code)
}

/// Tipo de cambio: unidades de la moneda por 1 € (como lo publica el BCE), en millonésimas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExchangeRate(i64);

impl ExchangeRate {
    /// Decimales con los que se guarda.
    pub const DECIMALS: usize = 6;
    const SCALE: i64 = 1_000_000;
    /// Tope razonable (1 € = 10 millones de unidades) para no desbordar.
    const MAX: i64 = 10_000_000 * Self::SCALE;

    pub fn from_micros(micros: i64) -> Option<Self> {
        (micros > 0 && micros <= Self::MAX).then_some(ExchangeRate(micros))
    }

    pub const fn micros(self) -> i64 {
        self.0
    }

    /// Lee un tipo escrito a mano o en una respuesta: `"1,1193"`, `"1.1193"`, `"0.83728"`,
    /// `"20.268,06"` (miles con punto si también hay coma). Como mucho 6 decimales; mayor que 0.
    pub fn parse(input: &str) -> Option<Self> {
        let t = input.trim();
        let (int, frac) = if t.contains(',') && t.contains('.') {
            let (i, f) = t.split_once(',')?;
            (i.replace('.', ""), f.to_owned())
        } else {
            match t.split_once([',', '.']) {
                Some((i, f)) => (i.to_owned(), f.to_owned()),
                None => (t.to_owned(), String::new()),
            }
        };
        let int = if int.is_empty() { "0".to_owned() } else { int };
        if !int.bytes().all(|b| b.is_ascii_digit())
            || !frac.bytes().all(|b| b.is_ascii_digit())
            || frac.len() > Self::DECIMALS
            || (int.len() > 12)
        {
            return None;
        }
        let i: i64 = int.parse().ok()?;
        let f: i64 = if frac.is_empty() {
            0
        } else {
            format!("{frac:0<6}").parse().ok()?
        };
        Self::from_micros(i.checked_mul(Self::SCALE)?.checked_add(f)?)
    }

    /// Importe en la moneda → euros (÷ tipo), al céntimo half away from zero.
    pub fn to_eur(self, amount: Money) -> Money {
        Money::from_cents(div_round(
            i128::from(amount.cents()) * i128::from(Self::SCALE),
            i128::from(self.0),
        ))
    }

    /// Euros → importe en la moneda (× tipo), al céntimo half away from zero.
    pub fn from_eur(self, eur: Money) -> Money {
        Money::from_cents(div_round(
            i128::from(eur.cents()) * i128::from(self.0),
            i128::from(Self::SCALE),
        ))
    }

    /// `"1.1193"`: punto decimal y sin ceros sobrantes (JSON, XML, la API del BCE).
    pub fn decimal(self) -> String {
        let (i, f) = (self.0 / Self::SCALE, self.0 % Self::SCALE);
        if f == 0 {
            return i.to_string();
        }
        let frac = format!("{f:06}");
        format!("{i}.{}", frac.trim_end_matches('0'))
    }

    /// `"1,1193"`: para mostrar en español.
    pub fn decimal_es(self) -> String {
        self.decimal().replace('.', ",")
    }
}

impl fmt::Display for ExchangeRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.decimal_es())
    }
}

impl Serialize for ExchangeRate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.decimal())
    }
}

impl<'de> Deserialize<'de> for ExchangeRate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ExchangeRate::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("tipo de cambio no válido: {s}")))
    }
}

/// División entera redondeando la mitad alejándose de cero. `den > 0`. Satura en vez de
/// desbordar (importes imposibles en una factura).
fn div_round(num: i128, den: i128) -> i64 {
    let q = num / den;
    let r = num % den;
    let q = if 2 * r.abs() >= den {
        q + num.signum()
    } else {
        q
    };
    i64::try_from(q).unwrap_or(if q < 0 { i64::MIN } else { i64::MAX })
}

/// De dónde sale el tipo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RateSource {
    /// Escrito por el usuario.
    #[default]
    Manual,
    /// Tipo de referencia del BCE descargado con «Tipo del BCE».
    Bce,
}

/// Moneda de una factura que no está en euros, con el tipo aplicado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignCurrency {
    /// ISO 4217 (`USD`, `GBP`…).
    pub code: String,
    /// Unidades de la moneda por 1 €.
    pub rate: ExchangeRate,
    /// Fecha del tipo (la de su publicación por el BCE / Banco de España).
    pub rate_date: Date,
    #[serde(default)]
    pub source: RateSource,
}

/// Días que el tipo puede ser anterior al devengo: el BCE no publica los fines de semana ni los
/// festivos de TARGET (1 de enero, Viernes Santo, Lunes de Pascua, 1 de mayo, 25 y 26 de
/// diciembre); el hueco más largo es de jueves a martes.
pub const MAX_RATE_AGE_DAYS: i64 = 7;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FxError {
    #[error("moneda no admitida: {0} (sólo monedas con dos decimales de la lista)")]
    Unsupported(String),
    #[error(
        "el tipo de cambio es del {rate_date} y la operación del {accrual}: se aplica el tipo \
         vigente al devengo, no uno posterior (art. 79.Once Ley 37/1992)"
    )]
    RateAfterAccrual { rate_date: Date, accrual: Date },
    #[error(
        "el tipo de cambio es del {rate_date} y la operación del {accrual}: usa el vigente al \
         devengo (art. 79.Once Ley 37/1992); «Tipo del BCE» lo descarga"
    )]
    StaleRate { rate_date: Date, accrual: Date },
}

impl ForeignCurrency {
    /// Moneda admitida (y no el euro).
    pub fn validate(&self) -> Result<(), FxError> {
        if currency(&self.code).is_none() {
            return Err(FxError::Unsupported(self.code.clone()));
        }
        Ok(())
    }

    /// El tipo es el vigente al devengo `accrual`: no posterior, ni más antiguo que
    /// [`MAX_RATE_AGE_DAYS`] (el último publicado antes de un fin de semana o festivo).
    pub fn check_rate_date(&self, accrual: Date) -> Result<(), FxError> {
        if self.rate_date > accrual {
            return Err(FxError::RateAfterAccrual {
                rate_date: self.rate_date,
                accrual,
            });
        }
        if (accrual - self.rate_date).whole_days() > MAX_RATE_AGE_DAYS {
            return Err(FxError::StaleRate {
                rate_date: self.rate_date,
                accrual,
            });
        }
        Ok(())
    }

    pub fn to_eur(&self, amount: Money) -> Money {
        self.rate.to_eur(amount)
    }
}

/// Líneas y totales en euros de una factura en moneda extranjera.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// Una por cada línea de la factura, en el mismo orden: cantidad 1 y precio = su base en
    /// euros (con el ajuste de redondeo del grupo).
    pub lines: Vec<InvoiceLine>,
    /// Totales en euros: base de cada grupo convertida y cuotas sobre ella.
    pub totals: Totals,
}

/// Pasa a euros las líneas de una factura en la moneda de `rate` (ver la documentación del
/// módulo).
pub fn convert(lines: &[InvoiceLine], rate: ExchangeRate) -> Converted {
    let bases: Vec<Money> = lines.iter().map(line_base).collect();
    let mut eur: Vec<Money> = bases.iter().map(|b| rate.to_eur(*b)).collect();
    let mut groups: BTreeMap<(VatTreatment, Option<Rate>), Vec<usize>> = BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        groups.entry((l.vat, l.surcharge)).or_default().push(i);
    }
    for idx in groups.values() {
        let target = rate.to_eur(idx.iter().map(|&i| bases[i]).sum());
        let sum: Money = idx.iter().map(|&i| eur[i]).sum();
        let diff = target - sum;
        if diff.is_zero() {
            continue;
        }
        // La primera de mayor importe (en valor absoluto).
        let mut j = idx[0];
        for &i in idx {
            if eur[i].abs() > eur[j].abs() {
                j = i;
            }
        }
        eur[j] += diff;
    }
    let lines: Vec<InvoiceLine> = lines
        .iter()
        .zip(eur)
        .map(|(l, e)| InvoiceLine {
            description: l.description.clone(),
            quantity_milli: 1000,
            unit_price: e,
            discount: Rate::ZERO,
            vat: l.vat,
            surcharge: l.surcharge,
            withholding: l.withholding,
            advance: l.advance,
        })
        .collect();
    // Por tipo: la cuota es la base del grupo en euros × tipo (art. 79.Once + art. 12.1).
    let totals = compute_totals(&lines, RoundingMode::PerRate);
    Converted { lines, totals }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::{Exemption, NotSubject};
    use proptest::prelude::*;
    use time::macros::date;

    fn rate(s: &str) -> ExchangeRate {
        ExchangeRate::parse(s).unwrap()
    }

    fn line(qty_milli: i64, price: i64, vat: VatTreatment) -> InvoiceLine {
        InvoiceLine {
            description: "x".into(),
            quantity_milli: qty_milli,
            unit_price: Money::from_cents(price),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: None,
            advance: false,
        }
    }

    const IVA21: VatTreatment = VatTreatment::Taxed {
        rate: Rate::percent(21),
    };

    #[test]
    fn lee_y_escribe_tipos() {
        assert_eq!(rate("1,1193").micros(), 1_119_300);
        assert_eq!(rate("1.1193").micros(), 1_119_300);
        assert_eq!(rate(" 0.83728 ").micros(), 837_280);
        assert_eq!(rate("20268.06").micros(), 20_268_060_000);
        assert_eq!(rate("20.268,06").micros(), 20_268_060_000);
        assert_eq!(rate("7").micros(), 7_000_000);
        assert_eq!(rate(",5").micros(), 500_000);
        assert_eq!(rate("1.1193").decimal(), "1.1193");
        assert_eq!(rate("7").decimal(), "7");
        assert_eq!(rate("0.837280").decimal_es(), "0,83728");
        for bad in [
            "",
            "0",
            "0,000000",
            "-1,1",
            "1,1234567",
            "abc",
            "1e3",
            "1,2,3",
        ] {
            assert_eq!(ExchangeRate::parse(bad), None, "{bad}");
        }
        let json = serde_json::to_string(&rate("1,1193")).unwrap();
        assert_eq!(json, "\"1.1193\"");
        assert_eq!(
            serde_json::from_str::<ExchangeRate>("\"1,1193\"").unwrap(),
            rate("1.1193")
        );
        assert!(serde_json::from_str::<ExchangeRate>("\"0\"").is_err());
    }

    #[test]
    fn convierte_al_centimo() {
        let usd = rate("1.1193");
        // 1.000,00 USD / 1,1193 = 893,415... → 893,42 €
        assert_eq!(
            usd.to_eur(Money::from_euros(1000, 0)),
            Money::from_cents(89_342)
        );
        assert_eq!(
            usd.to_eur(-Money::from_euros(1000, 0)),
            -Money::from_cents(89_342)
        );
        // Mitad: 0,05 / 2 = 0,025 → 0,03 (half away from zero).
        assert_eq!(rate("2").to_eur(Money::from_cents(5)), Money::from_cents(3));
        assert_eq!(
            rate("2").to_eur(Money::from_cents(-5)),
            Money::from_cents(-3)
        );
        assert_eq!(
            usd.from_eur(Money::from_euros(100, 0)),
            Money::from_cents(11_193)
        );
    }

    #[test]
    fn fecha_del_tipo() {
        let fx = ForeignCurrency {
            code: "USD".into(),
            rate: rate("1.1193"),
            rate_date: date!(2026 - 10 - 23),
            source: RateSource::Bce,
        };
        assert!(fx.validate().is_ok());
        // Sábado: vale el del viernes.
        assert!(fx.check_rate_date(date!(2026 - 10 - 24)).is_ok());
        assert!(fx.check_rate_date(date!(2026 - 10 - 30)).is_ok());
        assert!(matches!(
            fx.check_rate_date(date!(2026 - 10 - 31)),
            Err(FxError::StaleRate { .. })
        ));
        assert!(matches!(
            fx.check_rate_date(date!(2026 - 10 - 22)),
            Err(FxError::RateAfterAccrual { .. })
        ));
        for bad in ["EUR", "JPY", "usd", "XXX"] {
            let f = ForeignCurrency {
                code: bad.into(),
                ..fx.clone()
            };
            assert!(f.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn base_por_grupo_y_cuota_en_euros() {
        // Tres líneas de 0,10 USD al 21 % y tipo 3: por línea 0,03 € × 3 = 0,09 €, pero la
        // base del grupo es 0,30 / 3 = 0,10 €: la diferencia va a la primera línea.
        let lines: Vec<_> = (0..3).map(|_| line(1000, 10, IVA21)).collect();
        let c = convert(&lines, rate("3"));
        assert_eq!(c.totals.base, Money::from_cents(10));
        assert_eq!(
            c.lines
                .iter()
                .map(|l| l.unit_price.cents())
                .collect::<Vec<_>>(),
            vec![4, 3, 3]
        );
        assert_eq!(c.totals.vat, Money::from_cents(2)); // 0,10 × 21 % = 0,021 → 0,02
        // Una factura típica a EE. UU.: no sujeta (N2), 2 líneas.
        let n2 = VatTreatment::NotSubject(NotSubject::N2);
        let c = convert(
            &[line(10_000, 12_000, n2), line(2_500, 9_999, n2)],
            rate("1.1193"),
        );
        // 1.200,00 + 249,98 (2,5 × 99,99 = 249,975 → 249,98) = 1.449,98 USD → 1.295,43 €
        assert_eq!(c.totals.base, Money::from_cents(129_543));
        assert_eq!(c.totals.vat, Money::ZERO);
        assert_eq!(c.totals.invoice_total, c.totals.base);
        // Cada línea en euros es 1 × base.
        assert!(c.lines.iter().all(|l| l.quantity_milli == 1000));
    }

    #[test]
    fn grupos_por_tratamiento_y_retencion() {
        let mut a = line(1000, 100_000, IVA21);
        a.withholding = Some(Rate::percent(15));
        let b = line(1000, 50_000, VatTreatment::Exempt(Exemption::E1));
        let mut adv = line(1000, -20_000, IVA21);
        adv.advance = true;
        let c = convert(&[a, b, adv], rate("0.83728"));
        assert_eq!(c.totals.breakdown.len(), 2);
        // Grupo 21 %: 800,00 GBP → 955,47 €; cuota 200,65 €.
        let g = &c.totals.breakdown[0];
        assert_eq!(g.base, Money::from_cents(95_547));
        assert_eq!(g.vat_amount, Money::from_cents(20_065));
        // El anticipo conserva su marca (va a la 438) con su base en euros.
        assert!(c.lines[2].advance);
        assert_eq!(c.lines[2].unit_price, Money::from_cents(-23_887));
        // Retención sobre la línea en euros: 1.194,34 × 15 % = 179,15.
        assert_eq!(c.totals.withholding, Money::from_cents(17_915));
    }

    proptest! {
        /// La base de cada grupo en euros es exactamente la base del grupo en la moneda
        /// convertida, y las líneas en euros la suman.
        #[test]
        fn grupos_cuadran(
            items in prop::collection::vec((1i64..50_000, -100_000i64..1_000_000, 0usize..3), 1..15),
            micros in 1_000i64..50_000_000_000,
        ) {
            let rates = [0, 1000, 2100];
            let lines: Vec<_> = items.iter().map(|&(q, p, r)| line(q, p, VatTreatment::Taxed { rate: Rate::from_bp(rates[r]) })).collect();
            let r = ExchangeRate::from_micros(micros).unwrap();
            let doc = compute_totals(&lines, RoundingMode::PerRate);
            let c = convert(&lines, r);
            prop_assert_eq!(c.totals.breakdown.len(), doc.breakdown.len());
            for (e, d) in c.totals.breakdown.iter().zip(&doc.breakdown) {
                prop_assert_eq!(e.vat, d.vat);
                prop_assert_eq!(e.base, r.to_eur(d.base));
                let rate = match e.vat { VatTreatment::Taxed { rate } => rate, _ => Rate::ZERO };
                prop_assert_eq!(e.vat_amount, e.base.apply_rate(rate));
            }
            prop_assert_eq!(c.totals.invoice_total, c.totals.base + c.totals.vat);
        }
    }
}
