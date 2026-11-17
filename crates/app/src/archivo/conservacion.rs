//! Hasta cuándo hay que conservar cada ejercicio (G38). No borra nada: sólo dice qué ejercicios
//! ya se podrían destruir y cuáles no.
//!
//! - **Mercantil** (art. 30.1 CCom): libros, correspondencia, documentación y justificantes del
//!   negocio, seis años desde el último asiento de los libros. Se cuenta desde el cierre del
//!   ejercicio (el último asiento es el de cierre).
//! - **Fiscal** (arts. 66.a y 67.1 LGT; art. 70.1 LGT y art. 19 RD 1619/2012 para las
//!   facturas): cuatro años desde el día siguiente al fin del plazo voluntario de la declaración
//!   del ejercicio. Sociedades: el del Impuesto sobre Sociedades, 25 días después de los seis
//!   meses siguientes al cierre (art. 124.1 LIS). Personas físicas: el de la Renta, 30 de junio
//!   del año siguiente.
//! - **Créditos fiscales** (art. 66 bis.2 LGT, art. 70.3 LGT, art. 26.5 LIS): de un ejercicio que
//!   generó bases imponibles negativas o deducciones que siguen pendientes, la Administración
//!   puede comprobarlas diez años desde el fin del plazo de su declaración, y después hay que
//!   seguir acreditándolas (liquidación y contabilidad depositada) mientras se compensen: se
//!   conservan mientras queden pendientes y, como mínimo, esos diez años.
//!
//! `TODO(VERIFY)`: un requerimiento, una comprobación o un recurso interrumpen la prescripción
//! (art. 68 LGT) y alargan el plazo fiscal: no se calcula aquí; se avisa en el texto.

use serde::Serialize;
use taxpain_core::Nif;
use taxpain_vault::Vault;
use time::{Date, Duration, Month};

use crate::accounting;
use crate::error::Result;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conservacion {
    pub ejercicio: i32,
    /// Último día del ejercicio.
    pub cierre: Date,
    /// Art. 30 CCom: seis años desde el cierre.
    pub fin_mercantil: Date,
    /// Arts. 66 y 67 LGT: cuatro años desde el fin del plazo voluntario de declarar.
    pub fin_fiscal: Date,
    /// Arts. 66 bis y 70.3 LGT: diez años desde el fin del plazo, si generó BIN o deducciones.
    pub fin_creditos: Option<Date>,
    /// Le quedan BIN o deducciones pendientes de aplicar (se conserva mientras tanto).
    pub creditos_pendientes: bool,
    /// Lo más tarde de todo; `None` mientras queden créditos pendientes.
    pub conservar_hasta: Option<Date>,
    /// Ya ha pasado todo: se podría destruir (TaxPain no borra nada).
    pub destruible: bool,
    /// El porqué en una frase.
    pub motivo: String,
    /// Último «Archivo del ejercicio» generado (fecha y huella), si hay.
    pub archivado: Option<Archivado>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Archivado {
    pub creado_en: String,
    pub fichero: String,
    pub sha256: String,
}

fn anos(d: Date, n: i32) -> Date {
    // El 29 de febrero pasa al 28 en un año que no es bisiesto.
    Date::from_calendar_date(d.year() + n, d.month(), d.day())
        .or_else(|_| Date::from_calendar_date(d.year() + n, d.month(), 28))
        .unwrap_or(d)
}

/// `n` meses después de `d`; si `d` es fin de mes, el fin del mes de destino (seis meses
/// después del 30 de junio es el 31 de diciembre).
fn meses(d: Date, n: u8) -> Date {
    let fin_de_mes = d.day() == d.month().length(d.year());
    let mut m = d.month();
    let mut y = d.year();
    for _ in 0..n {
        if m == Month::December {
            y += 1;
        }
        m = m.next();
    }
    let dia = if fin_de_mes {
        m.length(y)
    } else {
        d.day().min(m.length(y))
    };
    Date::from_calendar_date(y, m, dia).unwrap_or(d)
}

/// Fin del plazo voluntario de la declaración anual del ejercicio que cierra en `cierre`.
pub fn fin_plazo_declaracion(cierre: Date, persona_fisica: bool) -> Date {
    if persona_fisica {
        Date::from_calendar_date(cierre.year() + 1, Month::June, 30).unwrap_or(cierre)
    } else {
        meses(cierre, 6) + Duration::days(25)
    }
}

/// Ejercicios con algo en TaxPain: asientos, facturas emitidas o recibidas, o fechas guardadas.
fn ejercicios(v: &Vault) -> Result<Vec<i32>> {
    let mut stmt = v.connection().prepare(
        "SELECT DISTINCT y FROM (
           SELECT year AS y FROM fiscal_year
           UNION SELECT fiscal_year FROM journal_entry
           UNION SELECT CAST(substr(issue_date, 1, 4) AS INTEGER) FROM invoice_issued
           UNION SELECT CAST(substr(issue_date, 1, 4) AS INTEGER) FROM invoice_received
             WHERE issue_date IS NOT NULL)
         WHERE y IS NOT NULL AND y > 1900 ORDER BY y",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i32>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Orígenes (ejercicios) con BIN o deducciones pendientes según el último dato guardado de cada
/// uno (tabla de arrastres del 200).
fn con_creditos(v: &Vault) -> Result<(Vec<i32>, Vec<i32>)> {
    // Todos los orígenes con créditos alguna vez, y los que aún tienen algo pendiente.
    let mut stmt = v.connection().prepare(
        "SELECT a.origen, a.pendiente FROM is_arrastre a
         WHERE a.ejercicio = (SELECT max(b.ejercicio) FROM is_arrastre b
                              WHERE b.tipo = a.tipo AND b.origen = a.origen)",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i32>(0)?, r.get::<_, i64>(1)?)))?;
    let mut alguna = Vec::new();
    let mut pendientes = Vec::new();
    for row in rows {
        let (origen, pendiente) = row?;
        alguna.push(origen);
        if pendiente > 0 {
            pendientes.push(origen);
        }
    }
    Ok((alguna, pendientes))
}

fn archivados(v: &Vault) -> Result<Vec<(i32, Archivado)>> {
    let mut stmt = v.connection().prepare(
        "SELECT ejercicio, creado_en, fichero, sha256 FROM archivo_ejercicio ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i32>(0)?,
            Archivado {
                creado_en: r.get(1)?,
                fichero: r.get(2)?,
                sha256: r.get(3)?,
            },
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Plazo de conservación de cada ejercicio con datos, del más antiguo al más reciente.
pub fn conservacion(v: &Vault, today: Date) -> Result<Vec<Conservacion>> {
    let persona_fisica = crate::presentado_aeat::company_nif(v)
        .and_then(|n| Nif::parse(&n).ok())
        .is_some_and(|n| n.is_persona_fisica());
    let (con_alguna, pendientes) = con_creditos(v)?;
    let hechos = archivados(v)?;
    let mut out = Vec::new();
    for y in ejercicios(v)? {
        let fy = accounting::fiscal_year(v, y)?;
        let cierre = fy.end;
        let plazo = fin_plazo_declaracion(cierre, persona_fisica);
        let fin_mercantil = anos(cierre, 6);
        let fin_fiscal = anos(plazo, 4);
        let fin_creditos = con_alguna.contains(&y).then(|| anos(plazo, 10));
        let creditos_pendientes = pendientes.contains(&y);
        let hasta = [Some(fin_mercantil), Some(fin_fiscal), fin_creditos]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(fin_mercantil);
        let conservar_hasta = (!creditos_pendientes).then_some(hasta);
        let destruible = !creditos_pendientes && today > hasta;
        let motivo = if creditos_pendientes {
            "le quedan bases negativas o deducciones pendientes: se conserva mientras se apliquen \
             (arts. 66 bis y 70.3 LGT, art. 26.5 LIS)"
                .to_owned()
        } else if destruible {
            "han pasado los seis años del Código de Comercio y la prescripción fiscal (salvo que \
             un requerimiento o una comprobación la hayan interrumpido)"
                .to_owned()
        } else if Some(hasta) == fin_creditos {
            format!("generó créditos fiscales: diez años de comprobación, hasta el {hasta}")
        } else if hasta == fin_mercantil {
            format!("seis años del Código de Comercio (art. 30): hasta el {hasta}")
        } else {
            format!("prescripción fiscal (art. 66 LGT): hasta el {hasta}")
        };
        let archivado = hechos
            .iter()
            .rev()
            .find(|(e, _)| *e == y)
            .map(|(_, a)| a.clone());
        out.push(Conservacion {
            ejercicio: y,
            cierre,
            fin_mercantil,
            fin_fiscal,
            fin_creditos,
            creditos_pendientes,
            conservar_hasta,
            destruible,
            motivo,
            archivado,
        });
    }
    Ok(out)
}

/// Aviso del Panel: qué ejercicios ya se podrían destruir y cuáles hay que conservar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AvisoConservacion {
    pub destruibles: Vec<i32>,
    /// Ejercicios cerrados que aún hay que conservar y el plazo de cada uno.
    pub conservar: Vec<Conservacion>,
    /// Ejercicios ya cerrados sin «Archivo del ejercicio» generado.
    pub sin_archivo: Vec<i32>,
}

pub fn aviso(v: &Vault, today: Date) -> Result<AvisoConservacion> {
    let todo = conservacion(v, today)?;
    Ok(AvisoConservacion {
        destruibles: todo
            .iter()
            .filter(|c| c.destruible)
            .map(|c| c.ejercicio)
            .collect(),
        sin_archivo: todo
            .iter()
            .filter(|c| c.cierre < today && c.archivado.is_none() && !c.destruible)
            .map(|c| c.ejercicio)
            .collect(),
        conservar: todo.into_iter().filter(|c| !c.destruible).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::date;

    fn vault() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap()
    }

    #[test]
    fn el_plazo_del_is_es_25_dias_tras_seis_meses() {
        assert_eq!(
            fin_plazo_declaracion(date!(2025 - 12 - 31), false),
            date!(2026 - 07 - 25)
        );
        assert_eq!(
            fin_plazo_declaracion(date!(2025 - 06 - 30), false),
            date!(2026 - 01 - 25)
        );
        assert_eq!(
            fin_plazo_declaracion(date!(2025 - 12 - 31), true),
            date!(2026 - 06 - 30)
        );
    }

    #[test]
    fn seis_anos_mercantiles_y_cuatro_fiscales() {
        let v = vault();
        for y in [2015, 2019, 2025] {
            v.connection()
                .execute(
                    "INSERT INTO fiscal_year(year, start_date, end_date, closed) VALUES (?1, ?2, ?3, 1)",
                    (y, format!("{y}-01-01"), format!("{y}-12-31")),
                )
                .unwrap();
        }
        let hoy = date!(2026 - 11 - 17);
        let c = conservacion(&v, hoy).unwrap();
        let e2019 = c.iter().find(|c| c.ejercicio == 2019).unwrap();
        assert_eq!(e2019.fin_mercantil, date!(2025 - 12 - 31));
        assert_eq!(e2019.fin_fiscal, date!(2024 - 07 - 25));
        assert!(e2019.destruible, "2019 ya pasó los seis años");
        let e2025 = c.iter().find(|c| c.ejercicio == 2025).unwrap();
        assert_eq!(e2025.conservar_hasta, Some(date!(2031 - 12 - 31)));
        assert!(!e2025.destruible);
        let a = aviso(&v, hoy).unwrap();
        assert_eq!(a.destruibles, vec![2015, 2019]);
        assert_eq!(a.sin_archivo, vec![2025]);
    }

    #[test]
    fn una_bin_pendiente_alarga_la_conservacion() {
        let v = vault();
        v.connection()
            .execute_batch(
                "INSERT INTO fiscal_year(year, start_date, end_date, closed)
                   VALUES (2015, '2015-01-01', '2015-12-31', 1), (2016, '2016-01-01', '2016-12-31', 1);
                 INSERT INTO is_arrastre(tipo, origen, ejercicio, pendiente, nota, updated_at)
                   VALUES ('bin', 2015, 2026, 500000, '', '2026-07-01'),
                          ('bin', 2016, 2025, 100000, '', '2025-07-01'),
                          ('bin', 2016, 2026, 0, '', '2026-07-01');",
            )
            .unwrap();
        let c = conservacion(&v, date!(2026 - 11 - 17)).unwrap();
        let e2015 = &c[0];
        assert!(e2015.creditos_pendientes);
        assert_eq!(e2015.conservar_hasta, None);
        assert!(!e2015.destruible);
        let e2016 = &c[1];
        assert!(!e2016.creditos_pendientes, "ya se compensó entera");
        assert_eq!(e2016.fin_creditos, Some(date!(2027 - 07 - 25)));
        assert!(!e2016.destruible, "diez años de comprobación de la BIN");
    }
}
