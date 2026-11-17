//! Modelo 123: retenciones e ingresos a cuenta sobre determinados rendimientos del capital
//! mobiliario (IRPF) y sobre determinadas rentas (IS e IRNR con establecimiento permanente).
//! Autoliquidación trimestral, en los veinte primeros días naturales de abril, julio, octubre y
//! enero (art. 108.1 RIRPF; art. 68.1 RIS); mensual para quien tenga un volumen de operaciones
//! de más de 6.010.121,04 € (art. 71.3 RIVA). No hay que presentarla en negativo si en el
//! periodo no se pagaron rentas sometidas a retención (mismos artículos).
//!
//! Base legal: art. 101.4 LIRPF y art. 128.6.a LIS (retención del 19 %); art. 128.4.d LIS y
//! art. 61.p RIS (sin retención los dividendos con derecho a la exención del art. 21 LIS);
//! art. 94.1 RIRPF y art. 65.1 RIS (devengo: exigibilidad del dividendo); Orden EHA/3435/2007
//! (anexo II sustituido por la Orden HAC/56/2024, BOE de 31-1-2024, que desglosa «Dividendos y
//! otras rentas de participación en fondos propios de entidades» y «Resto de rentas» desde los
//! devengos de 2024).
//!
//! Diseño de registro: `DR123e24.xls` (ejercicio 2024 y siguientes, 600 posiciones), convertido
//! con `tools/aeat/dr2json.py` a `fixtures/aeat/dr/123.json`; las casillas [01]–[14] vienen
//! marcadas en el DR. Notas en `docs/research/SOCIOS_Y_ADMINISTRADORES.md`.
//!
//! Alcance: dividendos a socios residentes (columna «Dividendos…») y, en el «Resto de rentas»,
//! los intereses de los préstamos recibidos de prestamistas residentes que no son bancos
//! (Contabilidad → Préstamos). El resto de rentas que no salen de ahí (arrendamiento de
//! negocio…), la periodificación de rentas anteriores a 1999 ([10] y [11]) y las
//! complementarias ([13]) van a cero. Los dividendos a no residentes no van aquí, sino en el
//! 216/296 (IRNR).
//!
//! `TODO(VERIFY)`: «Número de rentas» se cuenta como el número de pagos con retención (un pago a
//! cada socio en cada reparto), como dicen las instrucciones del 216 para su casilla análoga
//! («número total de rentas sobre las que el declarante haya venido obligado a retener»); las
//! del 123 no lo precisan en la orden.

use serde::{Deserialize, Serialize};
use taxpain_core::Money;

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::m303::Boxes;
use crate::withholding::DeclarationTypeW;
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

pub const DR123_JSON: &str = include_str!("../../../fixtures/aeat/dr/123.json");

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR123_JSON))
}

/// Una renta con retención: perceptor, si es un dividendo (participación en fondos propios) o
/// del resto de rentas, base y retención practicada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renta123 {
    pub payee: String,
    pub dividend: bool,
    pub base: Money,
    pub withheld: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result123 {
    pub year: i32,
    /// «1T»…«4T» o «01»…«12».
    pub period: String,
    /// Casilla → importe ([04]–[14]).
    pub boxes: Boxes,
    /// «Número de rentas»: ([01], n), ([02], n), ([03], n).
    pub rentas: Vec<(u16, u64)>,
    pub declaration: DeclarationTypeW,
}

impl Result123 {
    pub fn get(&self, b: u16) -> Money {
        self.boxes.get(&b).copied().unwrap_or(Money::ZERO)
    }

    /// Resultado a ingresar [14].
    pub fn result(&self) -> Money {
        self.get(14)
    }
}

pub fn compute_123(year: i32, period: &str, rentas: &[Renta123], iban: bool) -> Result123 {
    let with = |dividend: bool| {
        rentas
            .iter()
            .filter(move |r| r.dividend == dividend && !r.withheld.is_zero())
    };
    let count = |dividend: bool| with(dividend).count() as u64;
    let base = |dividend: bool| with(dividend).map(|r| r.base).sum::<Money>();
    let withheld = |dividend: bool| with(dividend).map(|r| r.withheld).sum::<Money>();
    let (n1, n2) = (count(true), count(false));
    let mut boxes = Boxes::new();
    let (b4, b5) = (base(true), base(false));
    let (r7, r8) = (withheld(true), withheld(false));
    for (k, v) in [
        (4, b4),
        (5, b5),
        (6, b4 + b5),
        (7, r7),
        (8, r8),
        (9, r7 + r8),
        (10, Money::ZERO),
        (11, Money::ZERO),
        (12, r7 + r8),
        (13, Money::ZERO),
        (14, r7 + r8),
    ] {
        boxes.insert(k, v);
    }
    let result = r7 + r8;
    let declaration = match (result.cents() > 0, iban) {
        (false, _) => DeclarationTypeW::Negativa,
        (true, true) => DeclarationTypeW::Domiciliacion,
        (true, false) => DeclarationTypeW::Ingreso,
    };
    Result123 {
        year,
        period: period.to_owned(),
        boxes,
        rentas: vec![(1, n1), (2, n2), (3, n1 + n2)],
        declaration,
    }
}

/// Fichero del 123 (página 01 del DR, 600 posiciones, dentro del sobre `<T123…>`).
pub fn file_123(r: &Result123, who: &Declarant, iban: Option<&str>) -> Result<String, DrError> {
    let page = spec().page("01")?;
    let mut w = PageWriter::new(page);
    w.text(6, r.declaration.code())?
        .text(7, &who.nif.to_uppercase())?
        .text(8, &who.full_name())?
        .text(9, &r.year.to_string())?
        .text(10, &r.period)?;
    for (box_, n) in &r.rentas {
        let f = page.field_for_box(*box_)?.n;
        w.text(f, &n.to_string())?;
    }
    for (box_, v) in &r.boxes {
        w.amount_box(*box_, *v)?;
    }
    if let (Some(iban), DeclarationTypeW::Domiciliacion) = (iban, r.declaration) {
        w.text(27, iban)?;
    }
    Ok(envelope(
        "123",
        r.year,
        &r.period,
        SOFTWARE_VERSION,
        DEVELOPER_NIF,
        &[w.finish()?],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dr::PageReader;

    fn who() -> Declarant {
        Declarant {
            nif: "B12345674".into(),
            name: "Estudio Ejemplo SL".into(),
            given_name: None,
        }
    }

    #[test]
    fn el_dr_tiene_600_posiciones_y_las_casillas() {
        let p = spec().page("01").unwrap();
        assert_eq!(p.len(), 600);
        for b in 1..=14 {
            assert!(p.field_for_box(b).is_ok(), "falta [{b:02}]");
        }
    }

    #[test]
    fn dividendos_del_trimestre() {
        let r = |payee: &str, base: i64| Renta123 {
            payee: payee.into(),
            dividend: true,
            base: Money::from_euros(base, 0),
            withheld: Money::from_euros(base, 0).apply_rate(taxpain_core::Rate::percent(19)),
        };
        let exempt = Renta123 {
            payee: "B87654321".into(),
            dividend: true,
            base: Money::from_euros(3000, 0),
            withheld: Money::ZERO,
        };
        let res = compute_123(
            2026,
            "2T",
            &[r("12345678Z", 6000), r("00000000T", 4000), exempt],
            false,
        );
        assert_eq!(res.rentas, vec![(1, 2), (2, 0), (3, 2)]);
        assert_eq!(res.get(4), Money::from_euros(10_000, 0));
        assert_eq!(res.get(7), Money::from_euros(1900, 0));
        assert_eq!(res.result(), Money::from_euros(1900, 0));
        assert_eq!(res.declaration, DeclarationTypeW::Ingreso);

        let f = file_123(&res, &who(), None).unwrap();
        assert!(f.starts_with("<T123020262T0000><AUX>"));
        assert!(f.ends_with("</T123020262T0000>"));
        let start = f.find("<T12301000>").unwrap();
        let rec: String = f[start..].chars().take(600).collect();
        let rd = PageReader::new(spec().page("01").unwrap(), &rec).unwrap();
        assert_eq!(rd.raw(6).unwrap(), "I");
        assert_eq!(rd.raw(7).unwrap(), "B12345674");
        assert_eq!(rd.raw(10).unwrap(), "2T");
        assert_eq!(rd.raw(11).unwrap(), "000000000000002");
        assert_eq!(rd.amount_box(9).unwrap(), Some(Money::from_euros(1900, 0)));
        assert_eq!(rd.amount_box(14).unwrap(), Some(Money::from_euros(1900, 0)));
        assert!(rec.ends_with("</T12301000>"));
    }

    #[test]
    fn sin_rentas_es_negativa() {
        let res = compute_123(2026, "1T", &[], true);
        assert_eq!(res.declaration, DeclarationTypeW::Negativa);
        let f = file_123(&res, &who(), Some("ES9121000418450200051332")).unwrap();
        // Negativa: sin IBAN.
        assert!(!f.contains("ES9121000418450200051332"));
    }
}
