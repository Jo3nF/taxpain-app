//! Series y numeración correlativa (RD 1619/2012 art. 6.1.a: numeración correlativa dentro de
//! cada serie; las rectificativas en serie específica, art. 6.1.a 2.º y 7.1.a 4.º; las
//! simplificadas (art. 7) en series separadas de las completas (art. 6) cuando se expiden unas y
//! otras en el mismo año natural, art. 7.1.a último párrafo).

use serde::{Deserialize, Serialize};

/// Plantilla del número: `{prefix}`, `{year}` y `{seq}` (con ancho fijo `seq_width`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberFormat {
    pub template: String,
    pub seq_width: usize,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            template: "{prefix}{year}-{seq}".into(),
            seq_width: 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    pub code: String,
    pub prefix: String,
    pub format: NumberFormat,
    /// Reinicia la numeración cada año.
    pub yearly_reset: bool,
    pub rectificativa: bool,
    /// Serie reservada a las facturas simplificadas (art. 7 RD 1619/2012).
    #[serde(default)]
    pub simplificada: bool,
}

impl Series {
    pub fn ordinaria() -> Self {
        Series {
            code: "A".into(),
            prefix: "A".into(),
            format: NumberFormat::default(),
            yearly_reset: true,
            rectificativa: false,
            simplificada: false,
        }
    }

    pub fn rectificativas() -> Self {
        Series {
            code: "R".into(),
            prefix: "R".into(),
            rectificativa: true,
            ..Series::ordinaria()
        }
    }

    pub fn simplificadas() -> Self {
        Series {
            code: "S".into(),
            prefix: "S".into(),
            simplificada: true,
            ..Series::ordinaria()
        }
    }

    /// Número formateado para la secuencia `seq` del año `year`.
    pub fn format_number(&self, year: i32, seq: u32) -> String {
        self.format
            .template
            .replace("{prefix}", &self.prefix)
            .replace("{year}", &year.to_string())
            .replace(
                "{seq}",
                &format!("{seq:0width$}", width = self.format.seq_width),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_por_defecto_y_personalizado() {
        assert_eq!(Series::ordinaria().format_number(2026, 7), "A2026-0007");
        assert_eq!(
            Series::rectificativas().format_number(2026, 1),
            "R2026-0001"
        );
        let s = Series {
            format: NumberFormat {
                template: "{year}/{seq}".into(),
                seq_width: 3,
            },
            ..Series::ordinaria()
        };
        assert_eq!(s.format_number(2026, 12), "2026/012");
        assert_eq!(s.format_number(2026, 1234), "2026/1234");
    }

    #[test]
    fn serie_de_simplificadas_y_series_guardadas_sin_el_campo() {
        let s = Series::simplificadas();
        assert!(s.simplificada && !s.rectificativa);
        assert_eq!(s.format_number(2026, 3), "S2026-0003");
        // Series guardadas antes de existir el campo: no son de simplificadas.
        let old: Series = serde_json::from_str(
            r#"{"code":"A","prefix":"A","format":{"template":"{prefix}{year}-{seq}","seq_width":4},"yearly_reset":true,"rectificativa":false}"#,
        )
        .unwrap();
        assert!(!old.simplificada);
    }
}
