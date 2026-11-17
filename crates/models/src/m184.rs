//! Modelo 184: declaración informativa anual de las entidades en régimen de atribución de rentas
//! (comunidades de bienes, sociedades civiles sin personalidad jurídica a efectos del IS,
//! herencias yacentes…). Base legal: art. 90 LIRPF, art. 70 RIRPF; Orden HAP/2250/2015
//! (actualizada por la Orden HAC/1430/2025).
//!
//! Diseño de registro transcrito de `DR_Modelo_184_2025.pdf` en `fixtures/aeat/dr/184.json`, con
//! sus reglas y dudas en `184-notas.md`. Registros de 500 posiciones: tipo 1 (declarante), tipo 2
//! de entidad («E», uno por clave y subclave) y tipo 2 de miembro («S», uno por miembro, clave y
//! subclave).
//!
//! Alcance: entidad española con actividad económica en estimación directa (normal o
//! simplificada; clave D/01) o con alquileres de inmuebles en España (capital inmobiliario,
//! clave C/01), retenciones soportadas (K/02 y K/03) y su atribución a cada miembro por su
//! porcentaje de participación. `TODO(VERIFY)`: sin estimación objetiva, rentas del extranjero,
//! capital mobiliario, ganancias patrimoniales, deducciones, reducciones (C/03, D/03) ni miembros
//! no residentes; un único inmueble por registro C.

use serde::{Deserialize, Serialize};
use taxpain_core::Money;

use crate::Declarant;
use crate::dr::{DrError, PageWriter, Spec, alnum};

pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/184.json");

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

/// 100 % con cuatro decimales (el porcentaje de participación del registro S).
pub const FULL_PCT: u32 = 1_000_000;

/// Máximo de provisiones y gastos de difícil justificación por miembro en estimación directa
/// simplificada (art. 30.2.4.ª LIRPF): 2.000 €.
pub const MAX_HARD_TO_JUSTIFY: Money = Money::from_cents(200_000);

/// Miembro de la entidad (socio, comunero, heredero o partícipe), residente en España.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partner184 {
    pub nif: String,
    pub name: String,
    /// Código de provincia del domicilio (01-52).
    pub province: String,
    pub address: String,
    /// Participación en las rentas, en diezmilésimas de punto (33,3333 % = 333_333).
    pub pct: u32,
    /// Sigue siendo miembro el 31 de diciembre.
    pub member_at_end: bool,
    /// Días que ha sido miembro en el año.
    pub days: u16,
}

/// Actividad económica en estimación directa (clave D/01).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity184 {
    /// 1 normal, 2 simplificada.
    pub regime: u8,
    /// Tipo de actividad: 1 empresarial mercantil, 2 agrícola o ganadera, 3 otras empresariales,
    /// 4 profesional artística o deportiva, 5 restantes profesionales.
    pub kind: u8,
    /// Grupo o epígrafe del IAE (sin punto).
    pub iae: String,
    pub income: Money,
    /// Detalle de gastos deducibles: personal, consumos, tributos, arrendamientos,
    /// reparaciones, servicios profesionales, suministros, financieros, amortizaciones,
    /// provisiones y otros.
    pub expenses: [Money; 11],
    /// Criterio de cobros y pagos.
    pub cash: bool,
}

impl Activity184 {
    pub fn total_expenses(&self) -> Money {
        self.expenses.iter().copied().sum()
    }

    pub fn net(&self) -> Money {
        self.income - self.total_expenses()
    }
}

/// Alquiler de un inmueble en España (capital inmobiliario, clave C/01).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rent184 {
    pub income: Money,
    /// Detalle: intereses, conservación, pendientes de años anteriores, tributos, dudoso
    /// cobro, servicios de terceros, seguros, amortización del inmueble, de muebles y otros.
    pub expenses: [Money; 10],
    /// Situación (1 con referencia catastral salvo País Vasco y Navarra, 2 País Vasco,
    /// 3 Navarra, 4 sin referencia) y referencia catastral.
    pub situation: u8,
    pub cadastral_ref: String,
    /// Urbano (si no, rústico).
    pub urban: bool,
    /// Días arrendado en el año.
    pub days: u16,
}

impl Rent184 {
    pub fn total_expenses(&self) -> Money {
        self.expenses.iter().copied().sum()
    }

    pub fn net(&self) -> Money {
        self.income - self.total_expenses()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input184 {
    /// 1 sociedad civil, 2 comunidad de bienes, 3 herencia yacente, 4 comunidad de
    /// propietarios, 5 otras.
    pub entity_type: u8,
    /// 1 empresarial, 2 profesional, 3 tenencia de inmuebles, 4 de valores, 5 otras.
    pub main_activity: u8,
    pub turnover: Money,
    pub representative_nif: String,
    pub representative_name: String,
    pub activity: Option<Activity184>,
    pub rent: Option<Rent184>,
    /// Retenciones soportadas por la actividad (K/03) y por alquileres urbanos (K/02).
    pub withholding_activity: Money,
    pub withholding_rent: Money,
    pub partners: Vec<Partner184>,
}

/// Lo atribuido a un miembro en una clave y subclave.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribution {
    pub partner: usize,
    pub key: char,
    pub subkey: u8,
    pub amount: Money,
    /// Provisiones y gastos de difícil justificación (clave D en simplificada).
    pub hard_to_justify: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result184 {
    pub year: i32,
    pub attributions: Vec<Attribution>,
    pub warnings: Vec<String>,
}

/// Reparte `total` según los porcentajes; el último se lleva los céntimos del redondeo si los
/// porcentajes suman el 100 %.
fn shares(total: Money, partners: &[Partner184]) -> Vec<Money> {
    let full = partners.iter().map(|p| u64::from(p.pct)).sum::<u64>() == u64::from(FULL_PCT);
    let mut out = Vec::new();
    let mut given = Money::ZERO;
    for (i, p) in partners.iter().enumerate() {
        let m = if full && i + 1 == partners.len() {
            total - given
        } else {
            let x = i128::from(total.cents()) * i128::from(p.pct);
            let den = i128::from(FULL_PCT);
            let q = x / den;
            let q = if 2 * (x % den).abs() >= den {
                q + x.signum()
            } else {
                q
            };
            Money::from_cents(i64::try_from(q).unwrap_or(0))
        };
        given += m;
        out.push(m);
    }
    out
}

pub fn compute(year: i32, i: &Input184) -> Result184 {
    let mut warnings = Vec::new();
    let mut attributions = Vec::new();
    let total: u64 = i.partners.iter().map(|p| u64::from(p.pct)).sum();
    if i.partners.is_empty() {
        warnings.push("Faltan los socios o comuneros y su participación".into());
    } else if total != u64::from(FULL_PCT) {
        warnings.push(format!(
            "Los porcentajes de participación suman {},{:04} % y deberían sumar 100 %",
            total / 10_000,
            total % 10_000
        ));
    }
    let mut push = |key: char, subkey: u8, total: Money, hard: &dyn Fn(Money) -> Money| {
        if total.is_zero() {
            return;
        }
        for (n, m) in shares(total, &i.partners).into_iter().enumerate() {
            let h = hard(m);
            attributions.push(Attribution {
                partner: n,
                key,
                subkey,
                amount: m - h,
                hard_to_justify: h,
            });
        }
    };
    if let Some(a) = &i.activity {
        let simplified = a.regime == 2;
        // Simplificada: 5 % del rendimiento neto previo de cada miembro, hasta 2.000 €.
        let hard = |share: Money| {
            if simplified && share.cents() > 0 {
                share
                    .apply_rate(taxpain_core::Rate::percent(5))
                    .min(MAX_HARD_TO_JUSTIFY)
            } else {
                Money::ZERO
            }
        };
        push('D', if simplified { 2 } else { 1 }, a.net(), &hard);
    }
    if let Some(r) = &i.rent {
        push('C', 0, r.net(), &|_| Money::ZERO);
        if r.situation == 4 || (r.situation <= 3 && r.cadastral_ref.trim().is_empty()) {
            warnings.push("Falta la referencia catastral del inmueble alquilado".into());
        }
        warnings.push(
            "Alquiler: si es vivienda, la reducción del art. 23.2 LIRPF (clave C/03) no se \
             calcula; añádela en el formulario."
                .into(),
        );
    }
    push('K', 3, i.withholding_activity, &|_| Money::ZERO);
    push('K', 2, i.withholding_rent, &|_| Money::ZERO);
    Result184 {
        year,
        attributions,
        warnings,
    }
}

fn fit(s: &str, n: usize) -> String {
    alnum(s)
        .chars()
        .take(n)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Importe en dos campos (entera y decimal), sin signo.
fn amount(w: &mut PageWriter<'_>, int: u16, dec: u16, m: Money) -> Result<(), DrError> {
    let abs = m.cents().unsigned_abs();
    w.text(int, &(abs / 100).to_string())?
        .text(dec, &format!("{:02}", abs % 100))?;
    Ok(())
}

/// Importe con signo («N» si es negativo) en tres campos.
fn signed(w: &mut PageWriter<'_>, sign: u16, int: u16, dec: u16, m: Money) -> Result<(), DrError> {
    if m.cents() < 0 {
        w.text(sign, "N")?;
    }
    amount(w, int, dec, m)
}

fn entity_record<'a>(
    s: &'a Spec,
    year: i32,
    who: &Declarant,
    key: char,
    subkey: &str,
) -> Result<PageWriter<'a>, DrError> {
    let mut w = PageWriter::new(s.page("184-2E")?);
    w.text(3, &year.to_string())?
        .text(4, &who.nif.to_uppercase())?
        .text(5, &who.nif.to_uppercase())?
        .text(7, &fit(&who.full_name(), 40))?
        .text(9, &key.to_string())?
        .text(10, subkey)?;
    Ok(w)
}

/// Fichero del 184: tipo 1, registros de entidad y registros de miembros (CRLF entre ellos;
/// `TODO(VERIFY)`: separador).
pub fn file(r: &Result184, i: &Input184, who: &Declarant) -> Result<String, DrError> {
    let s = spec();
    let mut entity = Vec::new();
    if let Some(a) = &i.activity {
        let mut w = entity_record(s, r.year, who, 'D', "01")?;
        let iae: String = a.iae.chars().filter(char::is_ascii_digit).take(4).collect();
        w.text(12, &a.regime.to_string())?
            .text(13, &a.kind.to_string())?
            .text(14, &iae)?;
        signed(&mut w, 24, 25, 26, a.income)?;
        amount(&mut w, 27, 28, a.total_expenses())?;
        signed(&mut w, 29, 30, 31, a.net())?;
        for (k, m) in a.expenses.iter().enumerate() {
            let n = 45 + 2 * u16::try_from(k).unwrap_or(0);
            amount(&mut w, n, n + 1, (*m).max(Money::ZERO))?;
        }
        if a.cash {
            w.text(67, "X")?;
        }
        entity.push(w.finish()?);
    }
    if let Some(rent) = &i.rent {
        let mut w = entity_record(s, r.year, who, 'C', "01")?;
        signed(&mut w, 24, 25, 26, rent.income)?;
        amount(&mut w, 27, 28, rent.total_expenses())?;
        signed(&mut w, 29, 30, 31, rent.net())?;
        w.text(43, &rent.situation.to_string())?
            .text(44, &fit(&rent.cadastral_ref, 20))?;
        for (k, m) in rent.expenses.iter().enumerate() {
            let n = 68 + 2 * u16::try_from(k).unwrap_or(0);
            amount(&mut w, n, n + 1, (*m).max(Money::ZERO))?;
        }
        w.text(88, &rent.days.min(366).to_string())?;
        entity.push(w.finish()?);
    }
    for (sub, m) in [("03", i.withholding_activity), ("02", i.withholding_rent)] {
        if !m.is_zero() {
            let mut w = entity_record(s, r.year, who, 'K', sub)?;
            amount(&mut w, 41, 42, m)?;
            entity.push(w.finish()?);
        }
    }

    let mut members = Vec::new();
    for at in &r.attributions {
        let Some(p) = i.partners.get(at.partner) else {
            continue;
        };
        let mut w = PageWriter::new(s.page("184-2S")?);
        w.text(3, &r.year.to_string())?
            .text(4, &who.nif.to_uppercase())?
            .text(5, &p.nif.to_uppercase())?
            .text(7, &fit(&p.name, 40))?
            .text(9, &p.province)?
            .text(11, "1")?
            .text(13, &p.days.min(366).to_string())?
            .text(14, &(p.pct / 10_000).to_string())?
            .text(15, &format!("{:04}", p.pct % 10_000))?
            .text(16, &at.key.to_string())?
            .text(17, &format!("{:02}", at.subkey))?
            .text(23, &fit(&p.address, 40))?;
        if p.member_at_end {
            w.text(12, "X")?;
        }
        signed(&mut w, 18, 19, 20, at.amount)?;
        if !at.hard_to_justify.is_zero() {
            w.text(24, &at.hard_to_justify.cents().to_string())?;
        }
        if at.key == 'C'
            && let Some(rent) = &i.rent
        {
            w.text(25, if rent.urban { "1" } else { "2" })?
                .text(26, &rent.situation.to_string())?
                .text(28, "T")?
                .text(29, &(p.pct / 10_000).to_string())?
                .text(30, &format!("{:02}", (p.pct % 10_000) / 100))?
                .text(31, &rent.days.min(366).to_string())?;
            if rent.situation <= 3 {
                w.text(27, &fit(&rent.cadastral_ref, 20))?;
            }
        }
        members.push(w.finish()?);
    }

    let mut w = PageWriter::new(s.page("184-1")?);
    let name = fit(&who.full_name(), 40);
    w.text(3, &r.year.to_string())?
        .text(4, &who.nif.to_uppercase())?
        .text(5, &name)?
        .text(6, "T")?
        .text(8, &name)?
        .text(9, &format!("184{:04}{:06}", r.year, 1))?
        .text(13, &members.len().to_string())?
        .text(14, &i.entity_type.to_string())?
        .text(15, &i.main_activity.to_string())?
        .text(24, &i.representative_nif.to_uppercase())?
        .text(25, &fit(&i.representative_name, 40))?
        .text(26, &entity.len().to_string())?;
    amount(&mut w, 22, 23, i.turnover.max(Money::ZERO))?;
    let mut out = vec![w.finish()?];
    out.extend(entity);
    out.extend(members);
    Ok(out.join("\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    fn input() -> Input184 {
        let partner = |nif: &str, pct: u32| Partner184 {
            nif: nif.into(),
            name: format!("Comunero {nif}"),
            province: "46".into(),
            address: "Calle Mayor 1, Valencia".into(),
            pct,
            member_at_end: true,
            days: 365,
        };
        Input184 {
            entity_type: 2,
            main_activity: 1,
            turnover: eur(100_000),
            representative_nif: "12345678Z".into(),
            representative_name: "Española Española Carmen".into(),
            activity: Some(Activity184 {
                regime: 2,
                kind: 1,
                iae: "659.9".into(),
                income: eur(100_000),
                expenses: [
                    eur(20_000),
                    eur(30_000),
                    Money::ZERO,
                    eur(10_000),
                    Money::ZERO,
                    Money::ZERO,
                    Money::ZERO,
                    Money::ZERO,
                    Money::ZERO,
                    Money::ZERO,
                    Money::ZERO,
                ],
                cash: false,
            }),
            rent: None,
            withholding_activity: eur(300),
            withholding_rent: Money::ZERO,
            partners: vec![partner("12345678Z", 666_667), partner("00000000T", 333_333)],
        }
    }

    #[test]
    fn atribucion_y_fichero() {
        let i = input();
        let r = compute(2026, &i);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        // Rendimiento neto previo 40.000: 26.666,68 y 13.333,32; difícil justificación del 5 %:
        // 1.333,33 y 666,67.
        let d: Vec<_> = r.attributions.iter().filter(|a| a.key == 'D').collect();
        assert_eq!(d[0].hard_to_justify, Money::from_cents(133_333));
        assert_eq!(
            d[0].amount + d[0].hard_to_justify,
            Money::from_cents(2_666_668)
        );
        assert_eq!(d[1].hard_to_justify, Money::from_cents(66_667));
        assert_eq!(d[0].subkey, 2);
        let k: Money = r
            .attributions
            .iter()
            .filter(|a| a.key == 'K')
            .map(|a| a.amount)
            .sum();
        assert_eq!(k, eur(300));
        let f = file(
            &r,
            &i,
            &Declarant {
                nif: "E12345674".into(),
                name: "Comunidad Ejemplo CB".into(),
                given_name: None,
            },
        )
        .unwrap();
        let lines: Vec<&str> = f.split("\r\n").collect();
        // Tipo 1, D/01, K/03 y dos miembros × (D, K).
        assert_eq!(lines.len(), 1 + 2 + 4);
        assert!(lines.iter().all(|l| l.chars().count() == 500));
        // Posiciones del DR (de 1 a 500, en caracteres: la Ñ ocupa un byte en ISO-8859-1).
        let at = |l: &str, from: usize, to: usize| -> String {
            l.chars().skip(from - 1).take(to + 1 - from).collect()
        };
        let t1 = lines[0];
        assert!(t1.starts_with("11842026E12345674"));
        assert_eq!(at(t1, 136, 144), "000000004");
        assert_eq!(at(t1, 145, 146), "21");
        assert_eq!(at(t1, 221, 229), "000000002");
        let d01 = lines[1];
        assert_eq!(at(d01, 76, 79), "ED01");
        assert_eq!(at(d01, 82, 87), "216599");
        assert_eq!(at(d01, 177, 190), " 0000004000000");
        let s1 = lines[3];
        assert_eq!(at(s1, 76, 76), "S");
        assert_eq!(at(s1, 86, 92), "0666667");
        assert_eq!(at(s1, 93, 95), "D02");
    }

    #[test]
    fn tope_de_2000_por_miembro() {
        let mut i = input();
        if let Some(a) = i.activity.as_mut() {
            a.income = eur(200_000);
        }
        let r = compute(2026, &i);
        assert_eq!(r.attributions[0].hard_to_justify, eur(2_000));
    }

    #[test]
    fn porcentajes_que_no_suman_100() {
        let mut i = input();
        i.partners[1].pct = 300_000;
        assert!(compute(2026, &i).warnings[0].contains("deberían sumar 100"));
    }
}
