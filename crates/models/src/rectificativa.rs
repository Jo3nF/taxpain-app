//! Autoliquidación rectificativa y complementaria: las marcas del fichero.
//!
//! Desde 2025 la autoliquidación rectificativa (art. 120.3 LGT, en la redacción de la Ley
//! 13/2023; arts. 126 bis y siguientes del RGAT tras el RD 117/2024) sustituye, en los modelos
//! que ya la admiten, a la complementaria y a la solicitud de rectificación: sirve tanto para
//! ingresar más como para que se devuelva lo ingresado de más. En los demás modelos sigue la
//! complementaria clásica (sólo para ingresar más) y, si sale menos, la solicitud de
//! rectificación por la Sede.
//!
//! Qué cauce tiene un modelo no se adivina: lo dice su diseño de registro. [`marcas`] busca en
//! el DR los campos de la autoliquidación rectificativa (indicador, número de justificante de la
//! anterior y motivo) o, si no los tiene, los de la declaración complementaria. Si el DR no
//! tiene ninguno, TaxPain no marca el fichero (no se inventan posiciones).
//!
//! [`marcar`] escribe esas marcas en un fichero ya generado por el modelo, con lo ingresado por
//! la anterior en su casilla y el resultado recalculado.

use serde::Serialize;
use taxpain_core::Money;

use crate::dr::{self, DrError, Field, Kind, PageSpec, PageWriter, Spec};

/// Cauce de la corrección según el diseño de registro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cauce {
    /// Autoliquidación rectificativa: sube o baja el resultado (art. 120.3 LGT).
    Rectificativa,
    /// Complementaria clásica: sólo para ingresar más (art. 122.2 LGT).
    Complementaria,
}

/// Campos de las marcas en el DR (números de campo de su página).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marcas {
    pub cauce: Cauce,
    /// Hoja del DR en que están.
    pub pagina: String,
    /// «Autoliquidación rectificativa» o «Declaración complementaria».
    pub indicador: u16,
    /// Número de justificante de la autoliquidación anterior.
    pub justificante: u16,
    /// Rectificativa: motivo «Rectificaciones (excepto las del motivo siguiente)».
    pub motivo: Option<u16>,
    /// Rectificativa: «solicito dar de baja (o modificar) la domiciliación efectuada».
    pub baja_domiciliacion: Option<u16>,
}

fn desc(f: &Field) -> String {
    f.desc.to_lowercase()
}

fn marcas_en(page: &PageSpec) -> Option<Marcas> {
    let libre = |f: &&Field| f.constant.is_none();
    let justificante = |page: &PageSpec, palabra: &str| {
        page.fields
            .iter()
            .filter(libre)
            .find(|f| f.len == 13 && desc(f).contains("justificante") && desc(f).contains(palabra))
            .map(|f| f.n)
    };
    // Rectificativa: el indicador es un campo de una posición que dice «autoliquidación
    // rectificativa» y no es el del motivo ni el de la baja de la domiciliación.
    let indicador = page.fields.iter().filter(libre).find(|f| {
        let d = desc(f);
        f.len == 1
            && d.contains("autoliquidación rectificativa")
            && !d.contains("motivo")
            && !d.contains("solicito")
    });
    if let Some(ind) = indicador
        && let Some(j) = justificante(page, "anterior")
    {
        let motivo = page
            .fields
            .iter()
            .filter(libre)
            .find(|f| {
                let d = desc(f);
                f.len == 1 && d.contains("motivo") && d.contains("rectificaciones (excepto")
            })
            .map(|f| f.n);
        let baja = page
            .fields
            .iter()
            .filter(libre)
            .find(|f| f.len == 1 && desc(f).contains("solicito dar de baja"))
            .map(|f| f.n);
        return Some(Marcas {
            cauce: Cauce::Rectificativa,
            pagina: page.sheet.clone(),
            indicador: ind.n,
            justificante: j,
            motivo,
            baja_domiciliacion: baja,
        });
    }
    // Complementaria: «Declaración complementaria» de una posición (no el «indicador de página
    // complementaria», que es otra cosa: varias páginas del mismo tipo).
    let ind = page.fields.iter().filter(libre).find(|f| {
        let d = desc(f);
        f.len == 1 && d.contains("declaración complementaria") && !d.contains("página")
    })?;
    let j = justificante(page, "anterior")?;
    Some(Marcas {
        cauce: Cauce::Complementaria,
        pagina: page.sheet.clone(),
        indicador: ind.n,
        justificante: j,
        motivo: None,
        baja_domiciliacion: None,
    })
}

/// Las marcas de rectificativa (o, si no las tiene, de complementaria) del diseño `spec`.
pub fn marcas(spec: &Spec) -> Option<Marcas> {
    let all: Vec<Marcas> = spec.pages.iter().filter_map(marcas_en).collect();
    all.iter()
        .find(|m| m.cauce == Cauce::Rectificativa)
        .or_else(|| all.first())
        .cloned()
}

/// Casillas de la liquidación de cada modelo que cambian en la rectificativa o complementaria.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Liquidacion {
    /// Página (sufijo de la hoja) con las casillas del resultado.
    pub pagina: &'static str,
    /// Página con el tipo de declaración (campo 6 en todos estos diseños).
    pub pagina_tipo: &'static str,
    /// Resultado antes de descontar lo de la anterior ([69] del 303, [28] del 111…). `None`: el
    /// modelo no tiene casilla para él (el 202: [03] = 18 % × [01] − [02]).
    pub total: Option<u16>,
    /// Lo ingresado con las autoliquidaciones anteriores del mismo periodo.
    pub anteriores: u16,
    /// Devoluciones ya acordadas por la AEAT de las anteriores (303 [109], 200 [01584]).
    pub devueltas: Option<u16>,
    /// Otra casilla que se resta del resultado (303 [112], gasolinas del depósito fiscal).
    pub otros: Option<u16>,
    /// Resultado de la autoliquidación.
    pub resultado: u16,
    /// Importe de la rectificación: lo ingresado de más que se pide devolver (303 [111], 200
    /// [00866]).
    pub rectificacion: Option<u16>,
}

/// Casillas de la liquidación de `model` (303, 111, 115, 123, 130, 202 y 200).
pub fn liquidacion(model: &str) -> Option<Liquidacion> {
    let l = |pagina, total, anteriores, resultado| Liquidacion {
        pagina,
        pagina_tipo: pagina,
        total,
        anteriores,
        devueltas: None,
        otros: None,
        resultado,
        rectificacion: None,
    };
    Some(match model {
        // [71] = [69] − [70] + [109] − [112] (DR303e26v101, página 03).
        "303" => Liquidacion {
            pagina: "03",
            pagina_tipo: "01",
            total: Some(69),
            anteriores: 70,
            devueltas: Some(109),
            otros: Some(112),
            resultado: 71,
            rectificacion: Some(111),
        },
        // [30] = [28] − [29] (dr111e16v18: campos 39, 40 y 41).
        "111" => l("01", Some(28), 29, 30),
        // [05] = [03] − [04].
        "115" => l("01", Some(3), 4, 5),
        // [14] = [12] − [13].
        "123" => l("01", Some(12), 13, 14),
        // [19] = [17] − [18].
        "130" => l("01", Some(17), 18, 19),
        // [03] = 18 % × [01] − [02] (modalidad del art. 40.2 LIS).
        "202" => l("(1)", None, 2, 3),
        // [00621] = [01586] − [01578] + [01584] (DR200e25, página 14B). `TODO(VERIFY)`: la
        // fórmula no viene en el DR; es la del formulario de Sociedades WEB de 2025.
        "200" => Liquidacion {
            pagina: "DP200014B",
            pagina_tipo: "DP200001",
            total: Some(1586),
            anteriores: 1578,
            devueltas: Some(1584),
            otros: None,
            resultado: 621,
            rectificacion: Some(866),
        },
        _ => return None,
    })
}

/// Número del campo de la casilla `[box_]` en la página: en el 111 el DR no trae las casillas y
/// el campo `11 + n` es la [n].
fn campo(model: &str, page: &PageSpec, box_: u16) -> Result<u16, DrError> {
    match page.field_for_box(box_) {
        Ok(f) => Ok(f.n),
        Err(e) if model == "111" => {
            let n = box_ + 11;
            page.field(n).map(|_| n).map_err(|_| e)
        }
        Err(e) => Err(e),
    }
}

/// Lo que se escribe en el fichero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Datos {
    /// Número de justificante de la anterior (13 cifras).
    pub justificante: String,
    /// Tipo de declaración («I», «U», «N», «C», «D»…).
    pub tipo: String,
    /// Ingresado con las anteriores.
    pub anteriores: Money,
    /// Devoluciones ya acordadas de las anteriores.
    pub devueltas: Money,
    /// Resultado recalculado.
    pub resultado: Money,
    /// Lo ingresado de más que se pide devolver (sólo rectificativa).
    pub rectificacion: Money,
    /// Pedir la baja (o el cambio) de la domiciliación de la anterior.
    pub baja_domiciliacion: bool,
}

/// ¿Es un número de justificante de la AEAT? Trece cifras.
pub fn justificante_valido(s: &str) -> bool {
    s.len() == 13 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Valor de una marca: «X» en los alfanuméricos, «1» en los numéricos (el 200).
fn si(page: &PageSpec, n: u16) -> Result<&'static str, DrError> {
    Ok(match page.field(n)?.kind {
        Kind::Num | Kind::N => "1",
        Kind::An | Kind::A => "X",
    })
}

/// Escribe en `file` (generado por el modelo con el diseño `spec`) las marcas de la
/// rectificativa o complementaria, lo ingresado con la anterior y el resultado recalculado.
pub fn marcar(model: &str, spec: &Spec, file: &str, d: &Datos) -> Result<String, DrError> {
    let m = marcas(spec).ok_or_else(|| {
        DrError::Unsupported(format!(
            "el diseño de registro del {model} que tiene TaxPain no trae la marca de \
             rectificativa ni de complementaria"
        ))
    })?;
    let l = liquidacion(model)
        .ok_or_else(|| DrError::Unsupported(format!("el {model} no se rectifica en TaxPain")))?;
    if !justificante_valido(&d.justificante) {
        return Err(DrError::Unsupported(
            "el número de justificante de la anterior tiene que tener trece cifras".into(),
        ));
    }
    if m.cauce == Cauce::Complementaria && d.resultado.cents() <= 0 {
        return Err(DrError::Unsupported(
            "una complementaria sólo sirve para ingresar más".into(),
        ));
    }
    let mut out = dr::patch_page(spec, file, l.pagina_tipo, |w| {
        w.text(6, &d.tipo)?;
        Ok(())
    })?;
    let page = spec.page(l.pagina)?;
    out = dr::patch_page(spec, &out, l.pagina, |w| {
        w.amount(campo(model, page, l.anteriores)?, d.anteriores)?;
        if let Some(b) = l.devueltas {
            w.amount(campo(model, page, b)?, d.devueltas)?;
        }
        if let Some(b) = l.rectificacion {
            w.amount(campo(model, page, b)?, d.rectificacion)?;
        }
        w.amount(campo(model, page, l.resultado)?, d.resultado)?;
        Ok(())
    })?;
    let mpage = spec
        .pages
        .iter()
        .find(|p| p.sheet == m.pagina)
        .ok_or_else(|| DrError::NoPage(model.to_owned(), m.pagina.clone()))?;
    let suffix = m.pagina.clone();
    out = dr::patch_page(spec, &out, &suffix, |w: &mut PageWriter<'_>| {
        w.text(m.indicador, si(mpage, m.indicador)?)?;
        w.text(m.justificante, &d.justificante)?;
        if let Some(n) = m.motivo {
            w.text(n, si(mpage, n)?)?;
        }
        if d.baja_domiciliacion
            && let Some(n) = m.baja_domiciliacion
        {
            w.text(n, si(mpage, n)?)?;
        }
        Ok(())
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_303_y_el_200_tienen_rectificativa_y_los_demas_complementaria() {
        let m = marcas(crate::m303::spec()).unwrap();
        assert_eq!(
            (m.cauce, m.pagina.as_str(), m.indicador, m.justificante),
            (Cauce::Rectificativa, "DP30303", 31, 32)
        );
        assert_eq!((m.motivo, m.baja_domiciliacion), (Some(35), Some(33)));
        let m = marcas(crate::m200::spec()).unwrap();
        assert_eq!(
            (m.cauce, m.pagina.as_str(), m.indicador, m.justificante),
            (Cauce::Rectificativa, "DP200001", 22, 23)
        );
        assert_eq!((m.motivo, m.baja_domiciliacion), (Some(24), Some(26)));
        for (spec, ind, just) in [
            (crate::withholding::spec_111(), 42, 43),
            (crate::withholding::spec_115(), 17, 18),
            (crate::m123::spec(), 25, 26),
            (crate::m130::spec(), 31, 32),
            (crate::m202::spec(), 52, 53),
        ] {
            let m = marcas(spec).unwrap();
            assert_eq!(
                (m.cauce, m.indicador, m.justificante),
                (Cauce::Complementaria, ind, just),
                "{}",
                spec.model
            );
        }
    }

    /// Las casillas de la liquidación existen en la página que se dice de cada diseño.
    #[test]
    fn las_casillas_de_la_liquidacion_estan_en_su_pagina() {
        for (model, spec) in [
            ("303", crate::m303::spec()),
            ("111", crate::withholding::spec_111()),
            ("115", crate::withholding::spec_115()),
            ("123", crate::m123::spec()),
            ("130", crate::m130::spec()),
            ("202", crate::m202::spec()),
            ("200", crate::m200::spec()),
        ] {
            let l = liquidacion(model).unwrap();
            let page = spec.page(l.pagina).unwrap();
            spec.page(l.pagina_tipo).unwrap().field(6).unwrap();
            for b in [
                Some(l.anteriores),
                Some(l.resultado),
                l.devueltas,
                l.rectificacion,
            ]
            .into_iter()
            .flatten()
            {
                campo(model, page, b).unwrap_or_else(|e| panic!("{model} [{b}]: {e}"));
            }
        }
    }

    #[test]
    fn justificantes() {
        assert!(justificante_valido("3030000000017"));
        assert!(!justificante_valido("303000000001"));
        assert!(!justificante_valido("30300000000A7"));
    }
}
