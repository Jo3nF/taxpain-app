//! Perfil fiscal deducido de lo presentado: cuando la empresa no tiene el perfil guardado
//! (Calendario → Perfil), lo que se ve en sus declaraciones y en el certificado de situación
//! censal dice qué modelos le tocan. Lo usan la revisión (para los modelos que faltan) y el
//! formulario del perfil («Rellenar con lo presentado»), que lo trae para que el usuario lo
//! confirme y lo guarde: no se guarda solo.
//!
//! Criterio:
//! - **Persona**: por el NIF (DNI o NIE, y los K, L y M, son personas físicas; los demás,
//!   sociedades). Sin NIF, sociedad si hay algún 200 o 202 y persona física si hay algún 100,
//!   130 o 131.
//! - **Modelos**: los de los tres últimos ejercicios con alguna declaración (el más reciente y
//!   los dos anteriores). Un modelo que dejó de presentarse antes no cuenta: la empresa pudo
//!   dejar de tener trabajadores o el local alquilado.
//! - **Certificado de situación censal**: sus obligaciones periódicas cuentan si es de esos
//!   mismos años (o si no hay ninguna declaración).
//! - Cada obligación sale de su modelo o de su resumen anual: IVA del 303 (mensual si alguno es
//!   mensual; con SII entonces, RIVA arts. 62.6 y 71.3), 349 (su periodicidad), 347, 111 o 190,
//!   115 o 180, 123 o 193, 202, 232, 720, 721 y 369 (trimestral, ventanilla única; mensual,
//!   IOSS). Gran empresa sólo con el 111 mensual (el 303 mensual puede ser del REDEME).
//! - Persona física: módulos con el 131; si no, estimación directa, y sin pagos fraccionados
//!   (el 70 % de los ingresos con retención) si no presenta el 130 ni el 131.
//! - Lo que no se ve en las declaraciones queda como en el perfil más prudente: cierre el 31 de
//!   diciembre, sin comunidad autónoma ni festivos locales, sin trabajadores ni RETA, sin Renta
//!   fraccionada, sin IAE y sin avisos de opciones censales.
//!
//! La revisión, además, sólo pide un modelo que falta en los ejercicios con ese mismo modelo
//! presentado el año anterior, el mismo o el siguiente (ver `revision::modelos`): con un perfil
//! que nadie ha confirmado, un año antiguo sin ese modelo no es un «falta».

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use taxpain_core::calendar::{IrpfMethod, Periodicity, Persona, TaxpayerProfile, VatProfile};
use taxpain_vault::Vault;
use time::{Date, Month};

use crate::error::Result;

/// El perfil deducido y por qué.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PerfilDeducido {
    pub perfil: TaxpayerProfile,
    /// De dónde sale cada cosa («303 trimestral (2024 a 2026)», «el NIF es de una sociedad»…).
    pub motivos: Vec<String>,
}

/// Una declaración vista: modelo, ejercicio y periodo («1T», «01», «0A»…).
pub(crate) type Vista = (String, i32, String);

/// Lo que se sabe para deducir el perfil.
#[derive(Clone, Debug, Default)]
pub(crate) struct Datos {
    pub nif: Option<String>,
    pub vistas: Vec<Vista>,
    /// Certificado de situación censal más reciente: su fecha y los modelos de sus obligaciones.
    pub censo: Option<(Option<Date>, BTreeSet<&'static str>)>,
}

/// Lo que tiene TaxPain de la empresa: su NIF, los modelos guardados (Importar → Comparar), las
/// declaraciones vigentes que tiene la AEAT («Comprobar lo presentado») y el certificado de
/// situación censal guardado entre los documentos de la empresa.
pub(crate) fn datos(v: &Vault) -> Result<Datos> {
    let mut vistas: Vec<Vista> = crate::presentados::list(v, None)?
        .into_iter()
        .map(|f| (f.model, f.year, f.period.to_uppercase()))
        .collect();
    vistas.extend(
        crate::presentado_aeat::vigentes(v)?
            .into_iter()
            .map(|x| (x.model, x.year, x.period.to_uppercase())),
    );
    let censo = crate::documentos::list(v)?
        .into_iter()
        .filter(|d| d.kind == crate::documentos::DocKind::CertificadoSituacionCensal)
        .filter_map(|d| {
            let o = crate::oficiales::leer(&crate::documentos::text(v, d.id).ok()?);
            Some((o.fecha.or(d.date), o.modelos_obligados()))
        })
        .max_by_key(|(f, _)| *f);
    Ok(Datos {
        nif: crate::carpeta::own_nif(v),
        vistas,
        censo,
    })
}

/// El perfil deducido de lo que tiene TaxPain, o `None` si no hay nada de lo que deducirlo.
pub fn deducir(v: &Vault) -> Result<Option<PerfilDeducido>> {
    Ok(de(&datos(v)?))
}

/// ¿Es un NIF de persona física? (DNI o NIE, y los K, L y M.)
pub(crate) fn nif_de_persona_fisica(nif: &str) -> bool {
    nif.trim()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || "KLMXYZ".contains(c.to_ascii_uppercase()))
}

/// ¿Es mensual el periodo? («01»…«12».)
fn mensual(p: &str) -> bool {
    p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit()) && p != "00"
}

/// El perfil deducido de `d` (ver el criterio en el módulo).
pub(crate) fn de(d: &Datos) -> Option<PerfilDeducido> {
    let ultimo = d.vistas.iter().map(|(_, y, _)| *y).max();
    let censo = d
        .censo
        .as_ref()
        .filter(|(f, m)| {
            !m.is_empty()
                && match (ultimo, f) {
                    (Some(u), Some(f)) => f.year() >= u - 2,
                    (Some(_), None) => false,
                    (None, _) => true,
                }
        })
        .map(|(f, m)| (*f, m));
    if ultimo.is_none() && censo.is_none() {
        return None;
    }
    // Modelo → (años, periodos) en la ventana de los tres últimos ejercicios.
    let mut recientes: BTreeMap<&str, (BTreeSet<i32>, BTreeSet<&str>)> = BTreeMap::new();
    if let Some(u) = ultimo {
        for (m, y, p) in &d.vistas {
            if *y >= u - 2 {
                let e = recientes.entry(m.as_str()).or_default();
                e.0.insert(*y);
                e.1.insert(p.as_str());
            }
        }
    }
    let hay = |m: &str| recientes.contains_key(m);
    let en_censo = |m: &str| censo.is_some_and(|(_, c)| c.contains(m));
    let con_mensual = |m: &str| {
        recientes
            .get(m)
            .is_some_and(|(_, ps)| ps.iter().any(|p| mensual(p)))
    };
    let mut motivos = Vec::new();
    let visto = |m: &str, que: &str| {
        recientes
            .get(m)
            .map(|(años, _)| format!("{m}{que} ({})", crate::revision::modelos::años_texto(años)))
    };
    let censo_texto = censo.map(|(f, _)| match f {
        Some(f) => format!(
            "certificado de situación censal del {:02}/{:02}/{}",
            f.day(),
            u8::from(f.month()),
            f.year()
        ),
        None => "certificado de situación censal".to_owned(),
    });

    let persona = match d.nif.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if nif_de_persona_fisica(n) => {
            motivos.push("el NIF es de una persona física".to_owned());
            Persona::Fisica
        }
        Some(_) => {
            motivos.push("el NIF es de una sociedad".to_owned());
            Persona::Juridica
        }
        None if ["100", "130", "131"].iter().any(|m| hay(m))
            && !["200", "202"].iter().any(|m| hay(m)) =>
        {
            motivos.push("presenta la Renta o pagos fraccionados del IRPF".to_owned());
            Persona::Fisica
        }
        None => {
            motivos.push("sin NIF: se toma como sociedad".to_owned());
            Persona::Juridica
        }
    };
    let fisica = persona == Persona::Fisica;
    let mut p = if fisica {
        TaxpayerProfile::autonomo()
    } else {
        TaxpayerProfile {
            persona: Persona::Juridica,
            irpf_method: None,
            ..TaxpayerProfile::autonomo()
        }
    };
    p.iva = None;

    // IVA.
    if hay("303") || en_censo("303") {
        let mes = con_mensual("303");
        p.iva = Some(VatProfile {
            periodicity: if mes {
                Periodicity::Mensual
            } else {
                Periodicity::Trimestral
            },
            sii: mes,
        });
        motivos.push(
            visto(
                "303",
                if mes {
                    " mensual (con SII)"
                } else {
                    " trimestral"
                },
            )
            .or_else(|| censo_texto.as_ref().map(|c| format!("IVA en el {c}")))
            .unwrap_or_default(),
        );
    }
    let sii = p.iva.is_some_and(|i| i.sii());

    // Retenciones: el trimestral o su resumen anual, o el censo.
    let mut retiene = |trimestral: &str, anual: &str, que: &str| -> bool {
        let por = visto(trimestral, "")
            .or_else(|| visto(anual, ""))
            .or_else(|| {
                en_censo(trimestral)
                    .then(|| {
                        censo_texto
                            .as_ref()
                            .map(|c| format!("{trimestral} en el {c}"))
                    })
                    .flatten()
            });
        match por {
            Some(m) => {
                motivos.push(format!("{m}: {que}"));
                true
            }
            None => false,
        }
    };
    p.retiene_trabajo_profesionales = retiene("111", "190", "retenciones del trabajo");
    p.retiene_alquileres = retiene("115", "180", "retenciones de alquileres");
    p.retiene_capital_mobiliario = retiene("123", "193", "retenciones del capital mobiliario");
    p.retiene_no_residentes = retiene("216", "296", "retenciones a no residentes");
    if con_mensual("111") {
        p.gran_empresa = true;
        motivos.push("111 mensual: gran empresa".to_owned());
    }

    // Intracomunitarias y operaciones con terceros.
    if hay("349") {
        let mes = con_mensual("349");
        p.intracomunitarias = Some(if mes {
            Periodicity::Mensual
        } else {
            Periodicity::Trimestral
        });
        motivos.extend(visto("349", if mes { " mensual" } else { " trimestral" }));
    } else if en_censo("349") {
        p.intracomunitarias = Some(Periodicity::Trimestral);
        motivos.extend(censo_texto.as_ref().map(|c| format!("349 en el {c}")));
    }
    if hay("347") && !sii {
        p.obligado_347 = true;
        motivos.extend(visto("347", ""));
    }

    // Impuesto sobre Sociedades o IRPF.
    if fisica {
        if hay("131") {
            p.irpf_method = Some(IrpfMethod::Modulos);
            motivos.extend(visto("131", ": estimación objetiva (módulos)"));
        } else if hay("130") || en_censo("130") {
            p.irpf_method = Some(IrpfMethod::EstimacionDirecta);
            motivos.extend(visto("130", ": estimación directa"));
        } else if p.iva.is_some() {
            p.irpf_method = Some(IrpfMethod::EstimacionDirecta);
            p.exento_pagos_fraccionados = true;
            motivos.push(
                "sin 130 ni 131: estimación directa, sin pagos fraccionados (el 70 % de los \
                 ingresos con retención)"
                    .to_owned(),
            );
        } else {
            p.irpf_method = None;
        }
    } else {
        if hay("202") {
            p.pagos_fraccionados_is = true;
            motivos.extend(visto("202", ""));
        }
        motivos.extend(visto("200", ""));
        motivos.push(
            "cierre del ejercicio el 31 de diciembre (no se deduce: cámbialo si los estatutos \
             dicen otro)"
                .to_owned(),
        );
        p.cierre_ejercicio = (Month::December, 31);
    }

    // Lo demás que se ve en las declaraciones.
    if hay("232") && !fisica {
        p.operaciones_vinculadas = true;
        motivos.extend(visto("232", ""));
    }
    if hay("720") {
        p.bienes_extranjero = true;
        motivos.extend(visto("720", ""));
    }
    if hay("721") {
        p.monedas_virtuales_extranjero = true;
        motivos.extend(visto("721", ""));
    }
    if let Some((_, ps)) = recientes.get("369") {
        p.oss = ps.iter().any(|x| x.ends_with('T'));
        p.ioss = ps.iter().any(|x| mensual(x));
        motivos.extend(visto("369", ""));
    }
    motivos.retain(|m| !m.is_empty());
    Some(PerfilDeducido { perfil: p, motivos })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vistas(v: &[(&str, i32, &str)]) -> Vec<Vista> {
        v.iter()
            .map(|(m, y, p)| ((*m).to_owned(), *y, (*p).to_owned()))
            .collect()
    }

    fn trimestres(m: &str, y: i32) -> Vec<(String, i32, String)> {
        ["1T", "2T", "3T", "4T"]
            .iter()
            .map(|p| (m.to_owned(), y, (*p).to_owned()))
            .collect()
    }

    #[test]
    fn sociedad_con_303_111_202_y_200() {
        let mut v = Vec::new();
        for y in [2024, 2025] {
            v.extend(trimestres("303", y));
            v.extend(trimestres("111", y));
            v.extend(vistas(&[
                ("390", y, "0A"),
                ("190", y, "0A"),
                ("200", y, "0A"),
                ("202", y, "1P"),
                ("202", y, "2P"),
                ("202", y, "3P"),
            ]));
        }
        // El 115 dejó de presentarse hace años: no cuenta.
        v.extend(trimestres("115", 2019));
        let d = de(&Datos {
            nif: Some("B12345674".into()),
            vistas: v,
            censo: None,
        })
        .unwrap();
        let p = &d.perfil;
        assert_eq!(p.persona, Persona::Juridica);
        assert_eq!(p.irpf_method, None);
        assert_eq!(
            p.iva,
            Some(VatProfile {
                periodicity: Periodicity::Trimestral,
                sii: false
            })
        );
        assert!(p.retiene_trabajo_profesionales);
        assert!(!p.retiene_alquileres);
        assert!(!p.retiene_capital_mobiliario);
        assert!(p.pagos_fraccionados_is);
        assert!(!p.gran_empresa);
        assert!(!p.obligado_347);
        assert_eq!(p.intracomunitarias, None);
        assert_eq!(p.cierre_ejercicio, (Month::December, 31));
        assert!(d.motivos.contains(&"el NIF es de una sociedad".to_owned()));
        assert!(
            d.motivos
                .contains(&"303 trimestral (2024 a 2025)".to_owned()),
            "{:?}",
            d.motivos
        );
        assert!(d.motivos.iter().any(|m| m.starts_with("111 (2024 a 2025)")));
        assert!(d.motivos.contains(&"202 (2024 a 2025)".to_owned()));
    }

    #[test]
    fn autonomo_sin_130_y_303_mensual() {
        let mut v = Vec::new();
        for m in 1..=12 {
            v.push(("303".to_owned(), 2025, format!("{m:02}")));
        }
        v.extend(trimestres("349", 2025));
        let d = de(&Datos {
            nif: Some("12345678Z".into()),
            vistas: v,
            censo: None,
        })
        .unwrap();
        let p = &d.perfil;
        assert_eq!(p.persona, Persona::Fisica);
        assert_eq!(p.irpf_method, Some(IrpfMethod::EstimacionDirecta));
        assert!(p.exento_pagos_fraccionados);
        assert!(
            p.iva
                .is_some_and(|i| i.periodicity == Periodicity::Mensual && i.sii)
        );
        assert_eq!(p.intracomunitarias, Some(Periodicity::Trimestral));
        assert!(!p.gran_empresa, "el 303 mensual no dice gran empresa");
    }

    #[test]
    fn con_el_certificado_de_situacion_censal() {
        let d = de(&Datos {
            nif: Some("B12345674".into()),
            vistas: vistas(&[("303", 2025, "1T")]),
            censo: Some((
                Date::from_calendar_date(2025, Month::March, 3).ok(),
                ["303", "115"].into_iter().collect(),
            )),
        })
        .unwrap();
        assert!(d.perfil.retiene_alquileres);
        assert!(
            d.motivos
                .iter()
                .any(|m| m.contains("115 en el certificado de situación censal del 03/03/2025")),
            "{:?}",
            d.motivos
        );
        // Un certificado antiguo no cuenta.
        let d = de(&Datos {
            nif: Some("B12345674".into()),
            vistas: vistas(&[("303", 2025, "1T")]),
            censo: Some((
                Date::from_calendar_date(2019, Month::March, 3).ok(),
                ["115"].into_iter().collect(),
            )),
        })
        .unwrap();
        assert!(!d.perfil.retiene_alquileres);
    }

    #[test]
    fn sin_nada_no_hay_perfil() {
        assert_eq!(
            de(&Datos {
                nif: Some("B12345674".into()),
                ..Datos::default()
            }),
            None
        );
    }
}
