//! Modelos 111 (retenciones del trabajo y de actividades económicas) y 115 (retenciones de
//! arrendamientos de inmuebles urbanos), trimestrales o, para las grandes empresas, mensuales
//! (art. 108.1 RIRPF; el periodo, en [`crate::periodo`]).
//!
//! Base legal: arts. 99 y 101 LIRPF; arts. 75, 100 y 108 RIRPF; Orden EHA/586/2011 (111) y Orden
//! EHA/3435/2007 y modificaciones (115). Diseños de registro: `dr111e16v18.xls`,
//! `DR115e15v13.xls`.
//!
//! El DR del 111 no numera las casillas: siguen el orden del modelo ([01]–[30]), de modo que la
//! casilla k está en el campo 11 + k. El del 115 igual ([01]–[05] → campos 12–16).
//!
//! Fuera de alcance: rendimientos en especie, premios, ganancias forestales, cesión de derechos
//! de imagen y complementarias ([29]/[04] = 0).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use taxpain_core::Money;

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

pub const DR111_JSON: &str = include_str!("../../../fixtures/aeat/dr/111.json");
pub const DR115_JSON: &str = include_str!("../../../fixtures/aeat/dr/115.json");

/// Un pago con retención: perceptor (NIF), importe sujeto y retención practicada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub payee: String,
    pub base: Money,
    pub withheld: Money,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input111 {
    /// Rendimientos del trabajo dinerarios (nóminas).
    pub work: Vec<Payment>,
    /// Rendimientos de actividades económicas dinerarios (profesionales).
    pub professionals: Vec<Payment>,
}

/// N: negativa (sin retenciones), I: ingreso, U: domiciliación.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclarationTypeW {
    Ingreso,
    Domiciliacion,
    Negativa,
}

impl DeclarationTypeW {
    pub fn code(self) -> &'static str {
        match self {
            DeclarationTypeW::Ingreso => "I",
            DeclarationTypeW::Domiciliacion => "U",
            DeclarationTypeW::Negativa => "N",
        }
    }

    fn of(result: Money, iban: bool) -> Self {
        match (result.cents() > 0, iban) {
            (false, _) => DeclarationTypeW::Negativa,
            (true, true) => DeclarationTypeW::Domiciliacion,
            (true, false) => DeclarationTypeW::Ingreso,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultW {
    pub model: &'static str,
    pub year: i32,
    /// Trimestre (1–4) o mes (101–112): ver [`crate::periodo`].
    pub quarter: u8,
    /// Casilla → importe. Los «nº de perceptores» no son importes: van en `payees`.
    pub boxes: crate::m303::Boxes,
    /// Casilla del nº de perceptores → número.
    pub payees: Vec<(u16, u64)>,
    pub declaration: DeclarationTypeW,
}

impl ResultW {
    pub fn get(&self, b: u16) -> Money {
        self.boxes.get(&b).copied().unwrap_or(Money::ZERO)
    }

    pub fn result(&self) -> Money {
        match self.model {
            "115" => self.get(5),
            _ => self.get(30),
        }
    }
}

fn totals(ps: &[Payment]) -> (u64, Money, Money) {
    // Perceptores: no cuenta quien tiene importes que se anulan entre sí en el periodo (una
    // factura y su rectificativa). Sí cuenta quien sólo aparece sin importes: las nóminas del
    // diario del gestor ponen los importes en el primer empleado y el resto sólo para contarlos.
    let mut net: BTreeMap<&str, (Money, Money, bool)> = BTreeMap::new();
    for p in ps {
        let e = net.entry(p.payee.as_str()).or_default();
        e.0 += p.base;
        e.1 += p.withheld;
        e.2 |= !p.base.is_zero() || !p.withheld.is_zero();
    }
    let payees: BTreeSet<&str> = net
        .into_iter()
        .filter(|(_, (b, r, con_importes))| !*con_importes || !b.is_zero() || !r.is_zero())
        .map(|(n, _)| n)
        .collect();
    (
        payees.len() as u64,
        ps.iter().map(|p| p.base).sum(),
        ps.iter().map(|p| p.withheld).sum(),
    )
}

pub fn compute_111(year: i32, quarter: u8, i: &Input111, iban: bool) -> ResultW {
    let (nw, bw, rw) = totals(&i.work);
    let (np, bp, rp) = totals(&i.professionals);
    let mut boxes = crate::m303::Boxes::new();
    for (k, v) in [(2, bw), (3, rw), (8, bp), (9, rp)] {
        boxes.insert(k, v);
    }
    let sum = rw + rp;
    boxes.insert(28, sum);
    boxes.insert(30, sum);
    ResultW {
        model: "111",
        year,
        quarter,
        boxes,
        payees: vec![(1, nw), (7, np)],
        declaration: DeclarationTypeW::of(sum, iban),
    }
}

/// Casillas del 111 que se pueden fijar a mano: bases e importes de cada clase de renta y la
/// casilla de complementarias [29]; [28] y [30] se recalculan.
pub const ADJUSTABLE_111: [u16; 19] = [
    2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 23, 24, 26, 27, 29,
];
/// Del 115: base [02], retenciones [03] y complementarias [04]; [05] se recalcula.
pub const ADJUSTABLE_115: [u16; 3] = [2, 3, 4];

/// ¿Se puede fijar a mano la casilla `b` del 111 o del 115?
pub fn adjustable(model: &str, b: u16) -> bool {
    match model {
        "111" => ADJUSTABLE_111.contains(&b),
        "115" => ADJUSTABLE_115.contains(&b),
        _ => false,
    }
}

/// Aplica las casillas fijadas a mano y recalcula totales, resultado y tipo de declaración.
/// Devuelve los avisos.
pub fn adjust(r: &mut ResultW, adjustments: &crate::m303::Boxes, iban: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    for (k, v) in adjustments {
        if adjustable(r.model, *k) {
            r.boxes.insert(*k, *v);
            warnings.push(format!("Casilla [{k:02}] fijada a mano: {v}"));
        }
    }
    if warnings.is_empty() {
        return warnings;
    }
    let get = |b: &crate::m303::Boxes, k: u16| b.get(&k).copied().unwrap_or(Money::ZERO);
    let result = if r.model == "115" {
        let r5 = get(&r.boxes, 3) - get(&r.boxes, 4);
        r.boxes.insert(5, r5);
        r5
    } else {
        let r28: Money = [3u16, 6, 9, 12, 15, 18, 21, 24, 27]
            .iter()
            .map(|k| get(&r.boxes, *k))
            .sum();
        let r30 = r28 - get(&r.boxes, 29);
        r.boxes.insert(28, r28);
        r.boxes.insert(30, r30);
        r30
    };
    r.declaration = DeclarationTypeW::of(result, iban);
    warnings
}

pub fn compute_115(year: i32, quarter: u8, rents: &[Payment], iban: bool) -> ResultW {
    let (n, base, withheld) = totals(rents);
    let mut boxes = crate::m303::Boxes::new();
    boxes.insert(2, base);
    boxes.insert(3, withheld);
    boxes.insert(5, withheld);
    ResultW {
        model: "115",
        year,
        quarter,
        boxes,
        payees: vec![(1, n)],
        declaration: DeclarationTypeW::of(withheld, iban),
    }
}

pub fn spec_111() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR111_JSON))
}

pub fn spec_115() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR115_JSON))
}

/// Fichero del 111 o del 115 según `r.model`.
pub fn file(r: &ResultW, who: &Declarant, iban: Option<&str>) -> Result<String, DrError> {
    let (spec, iban_field) = match r.model {
        "115" => (spec_115(), 19),
        _ => (spec_111(), 45),
    };
    let period = crate::periodo::codigo(r.quarter);
    let mut w = PageWriter::new(spec.page("01")?);
    w.text(6, r.declaration.code())?
        .text(7, &who.nif)?
        .text(8, &who.name)?
        .text(9, who.given_name.as_deref().unwrap_or(""))?
        .text(10, &r.year.to_string())?
        .text(11, &period)?;
    for (box_, v) in &r.boxes {
        w.amount(11 + box_, *v)?;
    }
    for (box_, n) in &r.payees {
        w.text(11 + box_, &n.to_string())?;
    }
    if let (Some(iban), DeclarationTypeW::Domiciliacion) = (iban, r.declaration) {
        w.text(iban_field, iban)?;
    }
    Ok(envelope(
        r.model,
        r.year,
        &period,
        SOFTWARE_VERSION,
        DEVELOPER_NIF,
        &[w.finish()?],
    ))
}
