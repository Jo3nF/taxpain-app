//! Estado de cambios en el patrimonio neto (ECPN) del ejercicio, con la contabilidad (PGC, 3.ª y
//! 4.ª parte; PGC de PYMES):
//!
//! - A) Estado de ingresos y gastos reconocidos: el resultado de la cuenta de pérdidas y
//!   ganancias, los ingresos y gastos imputados directamente al patrimonio neto (grupos 8 y 9) y
//!   las transferencias a la cuenta de pérdidas y ganancias.
//! - B) Estado total de cambios en el patrimonio neto: por cada componente de los fondos propios
//!   (capital, prima, reservas, resultados, subvenciones…), el saldo inicial, los ingresos y
//!   gastos reconocidos, las operaciones con socios (ampliaciones y reducciones de capital,
//!   dividendos, participaciones propias), las otras variaciones (la distribución del resultado)
//!   y el saldo final.
//!
//! Obligación: en el modelo normal, las dos partes; en el abreviado, el estado abreviado (arts.
//! 254 y 257 LSC); en el de PYMES es voluntario desde el RD 602/2016. TODO(VERIFY): si en el
//! abreviado basta con el estado total de cambios (parte B) o hacen falta las dos partes.
//!
//! Las columnas son las del modelo normal; en el abreviado y en el de PYMES se agrupan igual.
//! Sólo hay un ejercicio por estado: el comparativo del año anterior sale del estado de ese año.

use serde::Serialize;
use taxpain_core::accounting::entry::Origin;
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;

use crate::accounting::{self, StoredEntry};
use crate::error::{AppError, Result};

/// Columnas del estado total (componentes del patrimonio neto) y sus cuentas.
pub const COLUMNAS: &[(&str, &[&str])] = &[
    ("Capital", &["100", "101", "102", "103", "104"]),
    ("Prima de emisión", &["110"]),
    ("Reservas", &["112", "113", "114", "115", "119"]),
    ("Participaciones propias", &["108", "109"]),
    ("Resultados de ejercicios anteriores", &["120", "121"]),
    ("Otras aportaciones de socios", &["118"]),
    ("Resultado del ejercicio", &["129"]),
    ("Dividendo a cuenta", &["557"]),
    ("Otros instrumentos de patrimonio", &["111"]),
    (
        "Ajustes por cambios de valor",
        &["133", "134", "135", "136", "137"],
    ),
    ("Subvenciones, donaciones y legados", &["130", "131", "132"]),
];

/// Filas del estado total: saldo inicial, movimientos y saldo final.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Fila {
    SaldoInicial,
    IngresosGastos,
    AumentoCapital,
    ReduccionCapital,
    Dividendos,
    ParticipacionesPropias,
    OtrasVariaciones,
    SaldoFinal,
}

impl Fila {
    pub const TODAS: [Fila; 8] = [
        Fila::SaldoInicial,
        Fila::IngresosGastos,
        Fila::AumentoCapital,
        Fila::ReduccionCapital,
        Fila::Dividendos,
        Fila::ParticipacionesPropias,
        Fila::OtrasVariaciones,
        Fila::SaldoFinal,
    ];

    pub fn etiqueta(self, year: i32) -> String {
        match self {
            Fila::SaldoInicial => format!("Saldo al inicio del ejercicio {year}"),
            Fila::IngresosGastos => "I. Total ingresos y gastos reconocidos".into(),
            Fila::AumentoCapital => "II.1. Aumentos de capital".into(),
            Fila::ReduccionCapital => "II.2. Reducciones de capital".into(),
            Fila::Dividendos => "II.3. Distribución de dividendos".into(),
            Fila::ParticipacionesPropias => {
                "II.4. Operaciones con participaciones propias (netas)".into()
            }
            Fila::OtrasVariaciones => "III. Otras variaciones del patrimonio neto".into(),
            Fila::SaldoFinal => format!("Saldo al final del ejercicio {year}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Linea {
    pub label: String,
    pub amount: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FilaTotal {
    pub fila: Fila,
    pub label: String,
    /// Un importe por columna de [`COLUMNAS`] (en positivo lo que aumenta el patrimonio).
    pub importes: Vec<Money>,
    pub total: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ecpn {
    pub year: i32,
    pub model: ModelKind,
    /// Obligatorio con el modelo elegido (en el de PYMES, voluntario).
    pub obligatorio: bool,
    pub nota: String,
    /// A) Estado de ingresos y gastos reconocidos.
    pub ingresos_gastos: Vec<Linea>,
    /// Nombres de las columnas de B).
    pub columnas: Vec<String>,
    /// B) Estado total de cambios en el patrimonio neto.
    pub filas: Vec<FilaTotal>,
}

impl Ecpn {
    /// Columnas con algún importe (para no imprimir las vacías).
    pub fn columnas_con_datos(&self) -> Vec<usize> {
        (0..self.columnas.len())
            .filter(|&c| self.filas.iter().any(|f| !f.importes[c].is_zero()))
            .collect()
    }

    pub fn fila(&self, f: Fila) -> Option<&FilaTotal> {
        self.filas.iter().find(|x| x.fila == f)
    }
}

fn columna(account: &str) -> Option<usize> {
    COLUMNAS
        .iter()
        .position(|(_, ps)| ps.iter().any(|p| account.starts_with(p)))
}

/// Transferencias a la cuenta de pérdidas y ganancias (en los grupos 8 y 9). TODO(VERIFY): la
/// lista de cuentas de transferencia (802, 812, 821, 84, 862, 892; 902, 912, 921, 962, 992).
fn es_transferencia(account: &str) -> bool {
    [
        "802", "812", "813", "821", "84", "862", "892", "902", "912", "913", "921", "962", "992",
    ]
    .iter()
    .any(|p| account.starts_with(p))
}

/// Asiento que mueve las subvenciones (130, 131, 132) directamente, sin los grupos 8 y 9 ni
/// los asientos de apertura, cierre o aplicación del resultado.
fn subvencion_directa(e: &StoredEntry) -> bool {
    let lines = &e.entry.lines;
    !matches!(
        e.entry.origin,
        Origin::Regularizacion | Origin::Cierre | Origin::Apertura | Origin::AplicacionResultado(_)
    ) && lines.iter().any(|l| {
        let a = l.account.as_str();
        a.starts_with("130") || a.starts_with("131") || a.starts_with("132")
    }) && !lines.iter().any(|l| {
        let a = l.account.as_str();
        a.starts_with('8') || a.starts_with('9')
    })
}

/// Parte A con los asientos del ejercicio.
fn parte_a(entries: &[StoredEntry]) -> Vec<Linea> {
    let mut resultado = Money::ZERO;
    let mut directos = Money::ZERO;
    let mut efecto_directos = Money::ZERO;
    let mut transferencias = Money::ZERO;
    let mut efecto_transf = Money::ZERO;
    for e in entries {
        if matches!(
            e.entry.origin,
            Origin::Regularizacion | Origin::Cierre | Origin::Apertura
        ) {
            continue;
        }
        if subvencion_directa(e) {
            // Subvención registrada directamente en la 13x (sin los grupos 8 y 9, como en el
            // PGC de PYMES y en `subvenciones`): con la 7 es una transferencia a resultados; sin
            // ella, un ingreso imputado al patrimonio neto. La 479 es su efecto impositivo.
            let toca_resultados = e
                .entry
                .lines
                .iter()
                .any(|l| l.account.as_str().starts_with('7'));
            let pn: Money = e
                .entry
                .lines
                .iter()
                .filter(|l| l.account.as_str().starts_with("13"))
                .map(|l| l.credit - l.debit)
                .sum();
            let diferido: Money = e
                .entry
                .lines
                .iter()
                .filter(|l| l.account.as_str().starts_with("479"))
                .map(|l| l.credit - l.debit)
                .sum();
            // Bruto = neto en la 13x + lo que va a la 479; efecto = − 479.
            if toca_resultados {
                transferencias += pn + diferido;
                efecto_transf -= diferido;
            } else {
                directos += pn + diferido;
                efecto_directos -= diferido;
            }
        }
        for l in &e.entry.lines {
            let a = l.account.as_str();
            let neto = l.credit - l.debit;
            if a.starts_with('6') || a.starts_with('7') {
                resultado += neto;
            } else if a.starts_with('8') || a.starts_with('9') {
                // 83: efecto impositivo; el de las transferencias, 836 y 837.
                let impuesto = a.starts_with("83");
                match (es_transferencia(a), impuesto) {
                    (true, _) => transferencias += neto,
                    (false, true) if a.starts_with("836") || a.starts_with("837") => {
                        efecto_transf += neto;
                    }
                    (false, true) => efecto_directos += neto,
                    (false, false) => directos += neto,
                }
            }
        }
    }
    let b = directos + efecto_directos;
    let c = transferencias + efecto_transf;
    vec![
        Linea {
            label: "A) Resultado de la cuenta de pérdidas y ganancias".into(),
            amount: resultado,
        },
        Linea {
            label: "B) Total ingresos y gastos imputados directamente en el patrimonio neto".into(),
            amount: b,
        },
        Linea {
            label: "   de los que, efecto impositivo".into(),
            amount: efecto_directos,
        },
        Linea {
            label: "C) Total transferencias a la cuenta de pérdidas y ganancias".into(),
            amount: c,
        },
        Linea {
            label: "   de las que, efecto impositivo".into(),
            amount: efecto_transf,
        },
        Linea {
            label: "TOTAL DE INGRESOS Y GASTOS RECONOCIDOS (A + B + C)".into(),
            amount: resultado + b + c,
        },
    ]
}

/// A qué fila va cada línea de un asiento que toca el patrimonio neto.
fn clasificar(e: &StoredEntry) -> Vec<(usize, Fila, Money)> {
    let lines = &e.entry.lines;
    let dividendo: Money = lines
        .iter()
        .filter(|l| l.account.starts_with("526"))
        .map(|l| l.credit - l.debit)
        .sum();
    let con_dividendo = !dividendo.is_zero() || lines.iter().any(|l| l.account.starts_with("557"));
    let aplicacion = matches!(e.entry.origin, Origin::AplicacionResultado(_));
    let regularizacion = matches!(e.entry.origin, Origin::Regularizacion);
    let mut out = Vec::new();
    for l in lines {
        let Some(c) = columna(l.account.as_str()) else {
            continue;
        };
        let neto = l.credit - l.debit;
        if neto.is_zero() {
            continue;
        }
        let a = l.account.as_str();
        let fila = if regularizacion || (a.starts_with("13") && !aplicacion) {
            // El resultado al 129 y las subvenciones y ajustes de valor (grupos 8 y 9 al 13).
            Fila::IngresosGastos
        } else if aplicacion {
            if a.starts_with("129") && !dividendo.is_zero() {
                // El 129 que se reparte: lo que va a dividendos y lo demás a reservas.
                out.push((c, Fila::Dividendos, -dividendo));
                let resto = neto + dividendo;
                if !resto.is_zero() {
                    out.push((c, Fila::OtrasVariaciones, resto));
                }
                continue;
            }
            Fila::OtrasVariaciones
        } else if con_dividendo {
            Fila::Dividendos
        } else if a.starts_with("108") || a.starts_with("109") {
            Fila::ParticipacionesPropias
        } else if a.starts_with("10") {
            if neto.cents() > 0 {
                Fila::AumentoCapital
            } else {
                Fila::ReduccionCapital
            }
        } else {
            Fila::OtrasVariaciones
        };
        out.push((c, fila, neto));
    }
    out
}

fn obligatorio(kind: ModelKind) -> (bool, &'static str) {
    match kind {
        ModelKind::Normal => (
            true,
            "Obligatorio en el modelo normal (art. 254 LSC y PGC, 3.ª parte): las dos partes.",
        ),
        ModelKind::Abreviado => (
            true,
            "Obligatorio en el modelo abreviado, con el formato abreviado (arts. 254 y 257 LSC; \
             PGC, 4.ª parte). [Comprueba si basta con el estado total de cambios.]",
        ),
        ModelKind::Pymes => (
            false,
            "Voluntario en el PGC de PYMES desde el RD 602/2016: puedes incluirlo en las \
             cuentas si lo quieres.",
        ),
    }
}

/// ECPN de `year` con el modelo `kind`.
pub fn ecpn(v: &Vault, year: i32, kind: ModelKind) -> Result<Ecpn> {
    let entries = accounting::journal(v, year)?;
    let ingresos_gastos = parte_a(&entries);
    let n = COLUMNAS.len();
    let mut m = vec![[Money::ZERO; 8]; n];
    let idx = |f: Fila| Fila::TODAS.iter().position(|x| *x == f).unwrap_or(0);
    for e in &entries {
        match e.entry.origin {
            Origin::Cierre => continue,
            Origin::Apertura => {
                for l in &e.entry.lines {
                    if let Some(c) = columna(l.account.as_str()) {
                        m[c][idx(Fila::SaldoInicial)] += l.credit - l.debit;
                    }
                }
            }
            _ => {
                for (c, f, a) in clasificar(e) {
                    m[c][idx(f)] += a;
                }
            }
        }
    }
    // Antes de regularizar, el resultado del ejercicio aún no está en la 129: va con su importe
    // de la cuenta de pérdidas y ganancias, como en el balance.
    if !entries
        .iter()
        .any(|e| matches!(e.entry.origin, Origin::Regularizacion))
    {
        let resultado = ingresos_gastos[0].amount;
        if let Some(c) = columna("129") {
            m[c][idx(Fila::IngresosGastos)] += resultado;
        }
    }
    for col in &mut m {
        let total: Money = col[..7].iter().copied().sum();
        col[7] = total;
    }
    let filas = Fila::TODAS
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let importes: Vec<Money> = m.iter().map(|col| col[i]).collect();
            FilaTotal {
                fila: *f,
                label: f.etiqueta(year),
                total: importes.iter().copied().sum(),
                importes,
            }
        })
        .collect();
    let (obligatorio, nota) = obligatorio(kind);
    Ok(Ecpn {
        year,
        model: kind,
        obligatorio,
        nota: nota.into(),
        ingresos_gastos,
        columnas: COLUMNAS.iter().map(|(l, _)| (*l).to_owned()).collect(),
        filas,
    })
}

fn titulo(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Normal => "Estado de cambios en el patrimonio neto",
        ModelKind::Abreviado => "Estado abreviado de cambios en el patrimonio neto",
        ModelKind::Pymes => "Estado de cambios en el patrimonio neto (PYMES, voluntario)",
    }
}

/// Párrafos del ECPN (para el PDF y el RTF): la parte A con sus importes y, de la B, cada fila con
/// su total y el desglose por las columnas que tienen importes.
pub fn paragraphs(v: &Vault, year: i32, kind: ModelKind) -> Result<Vec<Para>> {
    let e = ecpn(v, year, kind)?;
    let info = crate::companies::info(v)?;
    let mut p = vec![
        Para::Title(format!("{} del ejercicio {year}", titulo(kind))),
        Para::Text(format!(
            "{} · NIF {}",
            info.name,
            info.nif.as_deref().unwrap_or("[NIF]")
        )),
        Para::Text(e.nota.clone()),
        Para::Gap,
        Para::Bold(format!(
            "A) Estado {}de ingresos y gastos reconocidos",
            if kind == ModelKind::Abreviado {
                "abreviado "
            } else {
                ""
            }
        )),
    ];
    for l in &e.ingresos_gastos {
        p.push(Para::Row(l.label.clone(), l.amount.to_string()));
    }
    p.push(Para::Gap);
    p.push(Para::Bold(format!(
        "B) Estado {}total de cambios en el patrimonio neto",
        if kind == ModelKind::Abreviado {
            "abreviado "
        } else {
            ""
        }
    )));
    let cols = e.columnas_con_datos();
    for f in &e.filas {
        let vacia = f.importes.iter().all(|m| m.is_zero());
        if vacia && !matches!(f.fila, Fila::SaldoInicial | Fila::SaldoFinal) {
            continue;
        }
        p.push(Para::Row(f.label.clone(), f.total.to_string()));
        for &c in &cols {
            if !f.importes[c].is_zero() {
                p.push(Para::Row(
                    format!("   {}", e.columnas[c]),
                    f.importes[c].to_string(),
                ));
            }
        }
    }
    p.push(Para::Gap);
    p.push(Para::Text(
        "Ajustes por cambios de criterio y corrección de errores: ninguno [compruébalo]. El \
         saldo inicial es el de la apertura del ejercicio; las cifras del ejercicio anterior, \
         en el estado de ese año."
            .into(),
    ));
    Ok(p)
}

pub fn pdf(v: &Vault, year: i32, kind: ModelKind) -> Result<Vec<u8>> {
    let p = paragraphs(v, year, kind)?;
    pdf::text_document(&format!("ECPN {year}"), &[p]).map_err(AppError::Other)
}

pub fn rtf(v: &Vault, year: i32, kind: ModelKind) -> Result<String> {
    Ok(crate::memoria::to_rtf(&paragraphs(v, year, kind)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columnas_por_cuenta() {
        assert_eq!(columna("1000001"), Some(0));
        assert_eq!(columna("1130"), Some(2));
        assert_eq!(columna("129"), Some(6));
        assert_eq!(columna("5570"), Some(7));
        assert_eq!(columna("170"), None);
        assert!(es_transferencia("8400"));
        assert!(!es_transferencia("940"));
    }
}
