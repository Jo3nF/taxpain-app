//! Qué modelo de cuentas anuales toca: PYMES, abreviado o normal, con el activo, la cifra de
//! negocios y la plantilla media de los dos últimos ejercicios.
//!
//! Límites (dos de los tres, al cierre de dos ejercicios consecutivos; en el primer ejercicio
//! desde la constitución, al cierre de ese ejercicio):
//! - Balance, ECPN y memoria abreviados (arts. 257 y 261 LSC) y PGC de PYMES (art. 2 del RD
//!   1515/2007, redacción del RD 602/2016): activo ≤ 4.000.000 €, cifra de negocios
//!   ≤ 8.000.000 € y plantilla media ≤ 50.
//! - Cuenta de pérdidas y ganancias abreviada (art. 258 LSC): activo ≤ 11.400.000 €, cifra de
//!   negocios ≤ 22.800.000 € y plantilla media ≤ 250.
//! - Criterios de microempresa (art. 4 del RD 1515/2007): activo ≤ 1.000.000 €, cifra de
//!   negocios ≤ 2.000.000 € y plantilla media ≤ 10.
//!
//! TODO(VERIFY): la Directiva Delegada (UE) 2023/2775 sube los umbrales de las pequeñas empresas
//! un 25 %; si España la ha incorporado a la LSC, cambian los importes de arriba.
//!
//! Se pierde la facultad cuando se dejan de reunir las condiciones durante dos ejercicios
//! seguidos: con un año que sí y otro que no, se sigue en la situación de antes, que TaxPain no
//! conoce (lo avisa).

use serde::Serialize;
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::error::Result;

/// Límites de un modelo: activo, cifra de negocios y plantilla media (personas).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Limites {
    pub activo: Money,
    pub cifra_negocios: Money,
    pub plantilla: i64,
}

/// Balance y memoria abreviados; también el PGC de PYMES.
pub const ABREVIADO: Limites = Limites {
    activo: Money::from_euros(4_000_000, 0),
    cifra_negocios: Money::from_euros(8_000_000, 0),
    plantilla: 50,
};

/// Cuenta de pérdidas y ganancias abreviada (art. 258 LSC).
pub const PYG_ABREVIADA: Limites = Limites {
    activo: Money::from_euros(11_400_000, 0),
    cifra_negocios: Money::from_euros(22_800_000, 0),
    plantilla: 250,
};

/// Criterios específicos de microempresas (art. 4 RD 1515/2007).
pub const MICRO: Limites = Limites {
    activo: Money::from_euros(1_000_000, 0),
    cifra_negocios: Money::from_euros(2_000_000, 0),
    plantilla: 10,
};

/// Plantilla media del ejercicio según las nóminas (sin los administradores), en décimas de
/// persona: total y, de ella, el personal no fijo (contratos de duración inferior al año, clave 2
/// del 190).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Plantilla {
    pub decimas: i64,
    pub no_fijos_decimas: i64,
}

impl Plantilla {
    pub fn fijos_decimas(self) -> i64 {
        self.decimas - self.no_fijos_decimas
    }
}

/// Décimas en texto: «2,5».
pub fn decimas(d: i64) -> String {
    format!("{},{}", d / 10, (d % 10).abs())
}

/// Plantilla media de `start`..`end` con las nóminas de TaxPain (`None` si no hay nóminas).
pub fn plantilla_media(v: &Vault, start: Date, end: Date) -> Result<Option<Plantilla>> {
    let (months, people, temporary): (i64, i64, i64) = v.connection().query_row(
        // Sin los administradores (no son personal empleado).
        "SELECT COUNT(DISTINCT substr(p.date, 1, 7)),
                COUNT(DISTINCT substr(p.date, 1, 7) || '-' || p.employee_id),
                COUNT(DISTINCT CASE WHEN e.contract = 2
                      THEN substr(p.date, 1, 7) || '-' || p.employee_id END)
           FROM payslip p JOIN employee e ON e.id = p.employee_id
          WHERE p.date BETWEEN ?1 AND ?2 AND e.administrator = 0",
        [start.to_string(), end.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if months == 0 {
        return Ok(None);
    }
    // Media de personas por mes del ejercicio (sus meses: el primero puede ser más corto).
    let span = (i64::from(end.year() - start.year()) * 12 + i64::from(u8::from(end.month()))
        - i64::from(u8::from(start.month()))
        + 1)
    .max(1);
    Ok(Some(Plantilla {
        decimas: people * 10 / span,
        no_fijos_decimas: temporary * 10 / span,
    }))
}

/// Datos de un ejercicio para los límites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ejercicio {
    pub year: i32,
    /// Hay asientos en el ejercicio (si no, no cuenta).
    pub con_datos: bool,
    pub activo: Money,
    pub cifra_negocios: Money,
    /// Plantilla media en décimas (0 si no hay nóminas).
    pub plantilla_decimas: i64,
    /// Cuántas de las tres condiciones cumple para el balance abreviado y PYMES, para la PyG
    /// abreviada y para microempresa.
    pub cumple_abreviado: u8,
    pub cumple_pyg_abreviada: u8,
    pub cumple_micro: u8,
}

fn cumple(l: Limites, activo: Money, cifra: Money, plantilla_decimas: i64) -> u8 {
    u8::from(activo <= l.activo)
        + u8::from(cifra <= l.cifra_negocios)
        + u8::from(plantilla_decimas <= l.plantilla * 10)
}

fn ejercicio(v: &Vault, year: i32) -> Result<Ejercicio> {
    let fy = accounting::fiscal_year(v, year)?;
    let con_datos = !accounting::journal(v, year)?.is_empty();
    let ca = accounting::annual_accounts_model(v, year, ModelKind::Pymes)?;
    let activo = ca.balance.get("TA");
    let cifra = ca.income.get("1");
    let plantilla = plantilla_media(v, fy.start, fy.end)?.unwrap_or_default();
    Ok(Ejercicio {
        year,
        con_datos,
        activo,
        cifra_negocios: cifra,
        plantilla_decimas: plantilla.decimas,
        cumple_abreviado: cumple(ABREVIADO, activo, cifra, plantilla.decimas),
        cumple_pyg_abreviada: cumple(PYG_ABREVIADA, activo, cifra, plantilla.decimas),
        cumple_micro: cumple(MICRO, activo, cifra, plantilla.decimas),
    })
}

/// ¿Se cumple (dos de tres) en los ejercicios con datos? `Some(true)` en todos, `Some(false)` en
/// ninguno, `None` si un año sí y otro no (sigue la situación anterior).
fn veredicto(ejs: &[&Ejercicio], f: impl Fn(&Ejercicio) -> u8) -> Option<bool> {
    let si = ejs.iter().filter(|e| f(e) >= 2).count();
    if si == ejs.len() {
        Some(true)
    } else if si == 0 {
        Some(false)
    } else {
        None
    }
}

/// Aviso del modelo que toca por tamaño, y el elegido en los ajustes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Aviso {
    pub year: i32,
    /// El ejercicio y el anterior (si tiene asientos).
    pub ejercicios: Vec<Ejercicio>,
    /// Modelo más sencillo que se puede usar (PYMES si caben los límites del art. 257).
    pub recomendado: ModelKind,
    /// Cabe el balance abreviado (y el PGC de PYMES); `None` si depende del ejercicio anterior.
    pub puede_abreviado: Option<bool>,
    /// Cabe la cuenta de pérdidas y ganancias abreviada (art. 258 LSC).
    pub puede_pyg_abreviada: Option<bool>,
    /// Puede aplicar los criterios de microempresa.
    pub puede_micro: Option<bool>,
    /// Modelo elegido en los ajustes (el de la memoria, el 200 y el depósito).
    pub elegido: ModelKind,
    /// Explicación en frases.
    pub explicacion: Vec<String>,
    /// Si el elegido no corresponde por tamaño, por qué.
    pub aviso: Option<String>,
}

fn nombre(k: ModelKind) -> &'static str {
    match k {
        ModelKind::Pymes => "PYMES",
        ModelKind::Abreviado => "abreviado",
        ModelKind::Normal => "normal",
    }
}

/// Qué modelo toca en `year` por los límites de los arts. 257 y 258 LSC y del RD 1515/2007.
pub fn aviso(v: &Vault, year: i32) -> Result<Aviso> {
    let elegido = crate::models::settings(v)?.accounts_model;
    let actual = ejercicio(v, year)?;
    let anterior = ejercicio(v, year - 1)?;
    let mut ejercicios = vec![actual];
    if anterior.con_datos {
        ejercicios.push(anterior);
    }
    let cuentan: Vec<&Ejercicio> = ejercicios.iter().collect();
    let puede_abreviado = veredicto(&cuentan, |e| e.cumple_abreviado);
    let puede_pyg_abreviada = veredicto(&cuentan, |e| e.cumple_pyg_abreviada);
    let puede_micro = veredicto(&cuentan, |e| e.cumple_micro);
    let mut explicacion = Vec::new();
    if cuentan.len() == 1 {
        explicacion.push(format!(
            "No hay asientos de {}: se toma sólo {year}, como en el primer ejercicio desde la \
             constitución (arts. 257.2 y 258.2 LSC). Si la sociedad es anterior, comprueba los \
             datos del año anterior.",
            year - 1
        ));
    }
    let recomendado = match puede_abreviado {
        Some(true) => {
            explicacion.push(
                "Reúne dos de las tres condiciones (activo hasta 4 millones, cifra de negocios \
                 hasta 8 millones y hasta 50 personas de media): puede formular el balance, el \
                 ECPN y la memoria abreviados (arts. 257 y 261 LSC) y aplicar el PGC de PYMES \
                 (RD 1515/2007), que es lo más sencillo."
                    .into(),
            );
            explicacion.push(
                "El PGC de PYMES no lo pueden aplicar las sociedades con valores admitidos a \
                 negociación, las que formulan cuentas consolidadas, las de moneda funcional \
                 distinta del euro ni las entidades financieras (art. 2 RD 1515/2007): en ese \
                 caso, el modelo abreviado."
                    .into(),
            );
            ModelKind::Pymes
        }
        Some(false) => {
            explicacion.push(
                "No reúne dos de las tres condiciones del balance abreviado: el balance, el ECPN \
                 y la memoria van en el modelo normal."
                    .into(),
            );
            ModelKind::Normal
        }
        None => {
            explicacion.push(
                "Un ejercicio cumple los límites del balance abreviado y el otro no: se sigue \
                 como se estaba antes (la facultad se gana o se pierde con dos ejercicios \
                 seguidos, art. 257.1 LSC). Elige el modelo que usaste el año pasado."
                    .into(),
            );
            elegido
        }
    };
    if recomendado == ModelKind::Normal && puede_pyg_abreviada == Some(true) {
        explicacion.push(
            "Sí puede formular la cuenta de pérdidas y ganancias abreviada (art. 258 LSC: activo \
             hasta 11,4 millones, cifra de negocios hasta 22,8 millones, hasta 250 personas)."
                .into(),
        );
    }
    if puede_micro == Some(true) {
        explicacion.push(
            "Puede aplicar los criterios específicos de microempresas (art. 4 RD 1515/2007: \
             activo hasta 1 millón, cifra de negocios hasta 2 millones, hasta 10 personas)."
                .into(),
        );
    }
    let aviso = match (elegido, puede_abreviado) {
        (ModelKind::Pymes | ModelKind::Abreviado, Some(false)) => Some(format!(
            "Tienes elegido el modelo {}, pero por tamaño toca el normal.",
            nombre(elegido)
        )),
        (ModelKind::Normal, Some(true)) => Some(
            "Tienes elegido el modelo normal: es válido, pero por tamaño puedes usar el de PYMES \
             o el abreviado, que piden menos información."
                .into(),
        ),
        _ => None,
    };
    Ok(Aviso {
        year,
        ejercicios,
        recomendado,
        puede_abreviado,
        puede_pyg_abreviada,
        puede_micro,
        elegido,
        explicacion,
        aviso,
    })
}

/// Guarda el modelo de cuentas de la sociedad (el de la memoria, el ECPN, el 200 y el depósito).
pub fn elegir(v: &Vault, kind: ModelKind, now: OffsetDateTime) -> Result<()> {
    let mut s = crate::models::settings(v)?;
    let antes = s.accounts_model;
    s.accounts_model = kind;
    crate::models::save_settings(v, &s, now)?;
    audit::append(
        v.connection(),
        &crate::invoicing::now_str(now),
        Actor::User,
        "cuentas.modelo",
        "models",
        &serde_json::json!({ "antes": nombre(antes), "despues": nombre(kind) }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_de_tres() {
        let m = |e| Money::from_euros(e, 0);
        assert_eq!(cumple(ABREVIADO, m(4_000_000), m(9_000_000), 500), 2);
        assert_eq!(cumple(ABREVIADO, m(4_000_001), m(9_000_000), 500), 1);
        assert_eq!(cumple(ABREVIADO, m(4_000_001), m(9_000_000), 501), 0);
        assert_eq!(cumple(PYG_ABREVIADA, m(5_000_000), m(9_000_000), 600), 3);
        assert_eq!(decimas(25), "2,5");
    }
}
