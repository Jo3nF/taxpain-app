//! El borrador de un modelo en una conversación con la IA: el tema que se envía (texto que genera
//! TaxPain; se anonimiza después, con lo demás) y la cabecera que ve el usuario en el panel.
//!
//! Van el resultado, las casillas (`[NN] importe`: sus nombres sólo están en la pantalla, y la IA
//! tiene `borrador_modelo`), los avisos y cuántos perceptores o terceros hay. Nunca la relación
//! de terceros (`details`: nombres, NIF e importes), el fichero ni su nombre (llevan el NIF y el
//! nombre del declarante), ni el CSV o el justificante de lo presentado.
//!
//! Los avisos pueden nombrar a un tercero («{nombre} ({NIF}): falta la provincia»): en la
//! conversación, sus nombres e identificadores (`Draft::terceros`) se tapan con marcadores, como
//! los datos de la empresa; donde no se puede (lo que devuelve una herramienta), se quitan
//! ([`sin_terceros`]).

use taxpain_core::money::Money;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Draft, FiledReturn, period_code};
use crate::conversaciones::TopicHeader;

/// Casillas que son un porcentaje (en centésimas), no un importe.
const PORCENTAJES: &[(&str, u16)] = &[("303", 65), ("200", 558)];

/// Una casilla como `[07] 4.700,00 €` (o `[65] 100,00 %` si es un porcentaje).
pub(crate) fn casilla(model: &str, k: u16, m: Money) -> String {
    if PORCENTAJES.contains(&(model, k)) {
        format!("[{k:02}] {}", m.to_string().replace(" €", " %"))
    } else {
        format!("[{k:02}] {m}")
    }
}

/// «3T 2026», «2P 2026» o, en los anuales, «2026».
pub(crate) fn periodo(model: &str, quarter: u8, year: i32) -> String {
    let code = period_code(model, quarter);
    if code == "0A" {
        year.to_string()
    } else {
        format!("{code} {year}")
    }
}

/// «del 3T 2026» o «de 2026».
fn del(model: &str, quarter: u8, year: i32) -> String {
    let p = periodo(model, quarter, year);
    if p == year.to_string() {
        format!("de {p}")
    } else {
        format!("del {p}")
    }
}

/// Qué es el resultado según el tipo de declaración (como lo dice la pantalla).
fn declaracion(code: &str) -> Option<&'static str> {
    Some(match code {
        "I" => "a ingresar",
        "U" => "a ingresar por domiciliación",
        "C" => "a compensar en los próximos trimestres",
        "D" => "a devolver",
        "N" => "sin actividad o resultado cero",
        "B" => "negativo: a deducir en los próximos pagos fraccionados",
        _ => return None,
    })
}

/// Qué es el importe destacado de los modelos sin tipo de declaración (los informativos, el 390
/// y el 200).
fn total(model: &str) -> Option<&'static str> {
    Some(match model {
        "349" => "Total de operaciones intracomunitarias",
        "390" => "Resultado de la liquidación anual",
        "200" => "Resultado (a ingresar si es positivo, a devolver si es negativo)",
        "347" => "Total de operaciones con terceros",
        "190" | "180" => "Total de retenciones",
        "184" => "Rendimiento neto de la entidad (se atribuye a los socios)",
        "193" => "Total de retenciones sobre dividendos",
        "296" => "Total de retenciones a no residentes",
        _ => return None,
    })
}

/// El día (dd/mm/aaaa, en la España peninsular) de un instante guardado.
fn dia(at: &str) -> String {
    match OffsetDateTime::parse(at, &Rfc3339) {
        Ok(t) => {
            let d = taxpain_core::spain_time::peninsular_date(t);
            format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
        }
        Err(_) => at.chars().take(10).collect(),
    }
}

/// `texto` sin los nombres ni los identificadores de `terceros`: cada uno, «un tercero» (sólo
/// palabras enteras, los más largos primero). Para lo que llega a la IA sin poder taparse con
/// marcadores.
pub(crate) fn sin_terceros(texto: &str, terceros: &[String]) -> String {
    let mut ts: Vec<&str> = terceros
        .iter()
        .map(|t| t.trim())
        .filter(|t| t.chars().count() >= 3)
        .collect();
    ts.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let palabra = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut out = texto.to_owned();
    for t in ts {
        let mut nuevo = String::with_capacity(out.len());
        let mut ultimo = 0;
        for (i, _) in out.match_indices(t) {
            let fin = i + t.len();
            if palabra(out[..i].chars().next_back()) || palabra(out[fin..].chars().next()) {
                continue;
            }
            nuevo.push_str(&out[ultimo..i]);
            nuevo.push_str("un tercero");
            ultimo = fin;
        }
        nuevo.push_str(&out[ultimo..]);
        out = nuevo;
    }
    out.replace("un tercero (un tercero)", "un tercero")
}

/// «1 aviso», «3 avisos».
fn avisos(n: usize) -> String {
    if n == 1 {
        "1 aviso".into()
    } else {
        format!("{n} avisos")
    }
}

/// El tema del borrador `d` y su cabecera. `presentado`: lo último que se presentó desde TaxPain
/// de ese modelo y periodo, si se presentó.
pub(crate) fn tema_borrador(d: &Draft, presentado: Option<&FiledReturn>) -> (TopicHeader, String) {
    let p = periodo(&d.model, d.quarter, d.year);
    let real = presentado.filter(|f| f.environment == "produccion");
    let pruebas = presentado.filter(|f| f.environment != "produccion");
    let que = declaracion(&d.declaration);

    let mut b = format!(
        "Borrador del modelo {} de TaxPain {}, calculado con las facturas y la contabilidad que \
         tiene TaxPain hoy (si se añaden o cambian las del periodo, cambia).",
        d.model,
        del(&d.model, d.quarter, d.year)
    );
    match (que, total(&d.model)) {
        (Some(q), _) => b.push_str(&format!(
            "\n- Resultado: {} ({q}; tipo de declaración {}).",
            d.result, d.declaration
        )),
        (None, Some(t)) => b.push_str(&format!("\n- {t}: {}.", d.result)),
        (None, None) => b.push_str(&format!("\n- Resultado: {}.", d.result)),
    }
    if !d.boxes.is_empty() {
        let casillas: Vec<String> = d
            .boxes
            .iter()
            .map(|(k, m)| casilla(&d.model, *k, *m))
            .collect();
        b.push_str(&format!("\n- Casillas: {}.", casillas.join("; ")));
    }
    if !d.payees.is_empty() {
        let perceptores: Vec<String> = d
            .payees
            .iter()
            .map(|(k, n)| format!("[{k:02}] {n}"))
            .collect();
        b.push_str(&format!(
            "\n- Número de perceptores: {}.",
            perceptores.join("; ")
        ));
    }
    if !d.details.is_empty() {
        b.push_str(&format!(
            "\n- Relación de terceros: {} (sus nombres, NIF e importes no se envían).",
            if d.details.len() == 1 {
                "1 línea".to_owned()
            } else {
                format!("{} líneas", d.details.len())
            }
        ));
    }
    if d.warnings.is_empty() {
        b.push_str("\n- Sin avisos de TaxPain.");
    } else {
        b.push_str("\n- Avisos de TaxPain:");
        for w in &d.warnings {
            b.push_str(&format!("\n  - {}", w.trim()));
        }
    }
    if let Some(f) = real {
        b.push_str(&format!(
            "\n- Ya presentado desde TaxPain el {} (resultado {}): cuenta lo presentado, no este \
             borrador.",
            dia(&f.presented_at),
            f.result
        ));
    } else if let Some(f) = pruebas {
        b.push_str(&format!(
            "\n- Presentado desde TaxPain sólo en el entorno de pruebas de la AEAT el {}: no \
             tiene efectos.",
            dia(&f.presented_at)
        ));
    }
    b.push_str(
        "\n- Pantalla de TaxPain donde se ve: Modelos (ahí se corrige una casilla, se prevalida \
         o se presenta, siempre con la confirmación del usuario; desde la conversación no se \
         presenta nada).",
    );

    let mut facts = vec![match (que, total(&d.model)) {
        (Some(q), _) => ("Resultado".to_owned(), format!("{} · {q}", d.result)),
        (None, Some(t)) => (t.to_owned(), d.result.to_string()),
        (None, None) => ("Resultado".to_owned(), d.result.to_string()),
    }];
    facts.push(("Periodo".to_owned(), p.clone()));
    if !d.warnings.is_empty() {
        facts.push(("Avisos".to_owned(), d.warnings.len().to_string()));
    }
    if let Some(f) = real {
        facts.push((
            "Presentado desde TaxPain".to_owned(),
            format!("el {}", dia(&f.presented_at)),
        ));
    } else if let Some(f) = pruebas {
        facts.push((
            "Presentado desde TaxPain".to_owned(),
            format!("sólo en pruebas, el {}", dia(&f.presented_at)),
        ));
    }

    let de = del(&d.model, d.quarter, d.year);
    let mut intro = match (que, total(&d.model)) {
        (Some(q), _) => format!(
            "Con tus facturas y tu contabilidad, el borrador del modelo {} {de} sale {} ({q}).",
            d.model, d.result
        ),
        (None, Some(t)) => format!(
            "Con tus facturas y tu contabilidad, el borrador del modelo {} {de} da {} ({}).",
            d.model,
            d.result,
            t.to_lowercase()
        ),
        (None, None) => format!(
            "Con tus facturas y tu contabilidad, el borrador del modelo {} {de} da {}.",
            d.model, d.result
        ),
    };
    if !d.warnings.is_empty() {
        intro.push_str(&format!(
            " Tiene {}: míralos antes de presentarlo.",
            avisos(d.warnings.len())
        ));
    }
    if real.is_some() {
        intro.push_str(" Ya está presentado desde TaxPain: cuenta lo presentado.");
    } else if pruebas.is_some() {
        intro.push_str(" Se presentó en el entorno de pruebas, que no tiene efectos.");
    }
    intro.push_str(
        " Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o \
         documentos).",
    );

    let primera = match d.declaration.as_str() {
        "I" | "U" => "¿Por qué me sale a ingresar?",
        "D" => "¿Por qué me sale a devolver?",
        "C" => "¿Por qué me sale a compensar?",
        "N" => "¿Por qué me sale a cero?",
        "B" => "¿Por qué me sale negativo?",
        _ => "¿Cómo se ha calculado el total?",
    };
    let suggestions = vec![
        primera.to_owned(),
        if d.boxes.is_empty() {
            "¿Qué datos lleva y de dónde salen?"
        } else {
            "Explícame las casillas principales"
        }
        .to_owned(),
        if real.is_some() {
            "¿Qué hago si tengo que corregir lo presentado?"
        } else {
            "¿Qué reviso antes de presentarlo?"
        }
        .to_owned(),
    ];

    (
        TopicHeader {
            group: format!("Modelos · {}", d.year),
            title: format!("Borrador del modelo {} · {p}", d.model),
            facts,
            intro,
            suggestions,
            notice: None,
        },
        b,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(c: i64) -> Money {
        Money::from_cents(c)
    }

    fn d303() -> Draft {
        Draft {
            model: "303".into(),
            year: 2026,
            quarter: 3,
            boxes: vec![
                (7, e(470_000)),
                (9, e(98_700)),
                (65, e(10_000)),
                (71, e(87_360)),
            ],
            payees: vec![],
            details: vec![],
            declaration: "I".into(),
            result: e(87_360),
            warnings: vec!["A1: tipo de IVA del 5 % sin casilla; revísalo a mano".into()],
            file_name: "B12345674_2026_3T_303.txt".into(),
            file: "<T30302026 3T B12345674 CLIENTE UNO SL".into(),
            terceros: vec![],
        }
    }

    /// Un 190 con la relación de perceptores (nombres y NIF de terceros).
    fn d190() -> Draft {
        Draft {
            model: "190".into(),
            year: 2025,
            quarter: 0,
            boxes: vec![],
            payees: vec![(1, 2)],
            details: vec![
                (
                    "Lucía Villalobos Cuesta · 12345678Z · G.01".into(),
                    e(15_000),
                ),
                ("Imprenta Ejemplo SL · B98765431 · G.01".into(), e(9_000)),
            ],
            declaration: String::new(),
            result: e(24_000),
            warnings: vec![],
            file_name: "B12345674_2025_190.txt".into(),
            file: "1190 2025 B12345674 CLIENTE UNO SL".into(),
            terceros: vec![
                "Lucía Villalobos Cuesta".into(),
                "12345678Z".into(),
                "Imprenta Ejemplo SL".into(),
                "B98765431".into(),
            ],
        }
    }

    fn presentado(environment: &str) -> FiledReturn {
        FiledReturn {
            id: 1,
            model: "303".into(),
            year: 2026,
            period: "3T".into(),
            environment: environment.into(),
            declaration: "I".into(),
            result: e(87_360),
            boxes: vec![],
            csv: "ABCD1234EFGH5678".into(),
            receipt_number: "3030000000001".into(),
            pdf_url: "https://prewww2.aeat.es/justificante".into(),
            presented_at: "2026-10-15T10:00:00Z".into(),
            has_receipt_pdf: false,
        }
    }

    #[test]
    fn bloque_del_borrador_sin_terceros_ni_fichero() {
        let (h, b) = tema_borrador(&d303(), None);
        assert!(
            b.starts_with(
                "Borrador del modelo 303 de TaxPain del 3T 2026, calculado con las facturas y la \
                 contabilidad que tiene TaxPain hoy (si se añaden o cambian las del periodo, \
                 cambia).\n- Resultado: 873,60 € (a ingresar; tipo de declaración I).\n- \
                 Casillas: [07] 4.700,00 €; [09] 987,00 €; [65] 100,00 %; [71] 873,60 €.\n- \
                 Avisos de TaxPain:\n  - A1: tipo de IVA del 5 % sin casilla; revísalo a mano\n"
            ),
            "{b}"
        );
        assert!(b.contains("desde la conversación no se presenta nada"));
        for fuera in ["B12345674", "CLIENTE UNO", ".txt", "<T303"] {
            assert!(!b.contains(fuera), "«{fuera}» en el tema:\n{b}");
        }
        assert_eq!(h.group, "Modelos · 2026");
        assert_eq!(h.title, "Borrador del modelo 303 · 3T 2026");
        assert_eq!(
            h.facts,
            [
                ("Resultado", "873,60 € · a ingresar"),
                ("Periodo", "3T 2026"),
                ("Avisos", "1"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        assert_eq!(
            h.intro,
            "Con tus facturas y tu contabilidad, el borrador del modelo 303 del 3T 2026 sale \
             873,60 € (a ingresar). Tiene 1 aviso: míralos antes de presentarlo. Pregunta lo que \
             quieras o añade datos que TaxPain no tenga (cifras, decisiones o documentos)."
        );
        assert_eq!(
            h.suggestions,
            [
                "¿Por qué me sale a ingresar?",
                "Explícame las casillas principales",
                "¿Qué reviso antes de presentarlo?"
            ]
        );

        // Un informativo: cuántos perceptores y cuántas líneas, nunca quiénes.
        let (h, b) = tema_borrador(&d190(), None);
        assert!(
            b.starts_with("Borrador del modelo 190 de TaxPain de 2025,"),
            "{b}"
        );
        assert!(b.contains("\n- Total de retenciones: 240,00 €.\n"));
        assert!(b.contains("\n- Número de perceptores: [01] 2.\n"));
        assert!(b.contains(
            "\n- Relación de terceros: 2 líneas (sus nombres, NIF e importes no se envían).\n"
        ));
        assert!(b.contains("\n- Sin avisos de TaxPain.\n"));
        for fuera in [
            "Villalobos",
            "12345678Z",
            "Imprenta",
            "B98765431",
            "150,00",
            "B12345674",
        ] {
            assert!(!b.contains(fuera), "«{fuera}» en el tema:\n{b}");
        }
        assert_eq!(h.title, "Borrador del modelo 190 · 2025");
        assert_eq!(
            h.facts[0],
            ("Total de retenciones".into(), "240,00 €".into())
        );
        assert_eq!(
            h.suggestions,
            [
                "¿Cómo se ha calculado el total?",
                "¿Qué datos lleva y de dónde salen?",
                "¿Qué reviso antes de presentarlo?"
            ]
        );
    }

    #[test]
    fn presentado_desde_taxpain_se_dice_sin_su_csv() {
        let real = presentado("produccion");
        let (h, b) = tema_borrador(&d303(), Some(&real));
        assert!(b.contains(
            "\n- Ya presentado desde TaxPain el 15/10/2026 (resultado 873,60 €): cuenta lo \
             presentado, no este borrador."
        ));
        for fuera in ["ABCD1234EFGH5678", "3030000000001", "prewww2"] {
            assert!(!b.contains(fuera), "«{fuera}» en el tema:\n{b}");
        }
        assert!(
            h.facts
                .contains(&("Presentado desde TaxPain".into(), "el 15/10/2026".into()))
        );
        assert!(
            h.intro
                .contains("Ya está presentado desde TaxPain: cuenta lo presentado.")
        );
        assert_eq!(
            h.suggestions[2],
            "¿Qué hago si tengo que corregir lo presentado?"
        );
        // En el entorno de pruebas no cuenta.
        let (h, b) = tema_borrador(&d303(), Some(&presentado("pruebas")));
        assert!(b.contains(
            "\n- Presentado desde TaxPain sólo en el entorno de pruebas de la AEAT el 15/10/2026: \
             no tiene efectos."
        ));
        assert!(!b.contains("cuenta lo presentado"));
        assert_eq!(h.suggestions[2], "¿Qué reviso antes de presentarlo?");
    }

    #[test]
    fn sin_terceros_quita_sus_nombres_e_identificadores() {
        let t = d190().terceros;
        assert_eq!(
            sin_terceros(
                "Lucía Villalobos Cuesta (12345678Z) retiene al 7 %: le corresponde la G.03",
                &t
            ),
            "un tercero retiene al 7 %: le corresponde la G.03"
        );
        assert_eq!(
            sin_terceros("Imprenta Ejemplo SL: falta la provincia del domicilio", &t),
            "un tercero: falta la provincia del domicilio"
        );
        assert_eq!(
            sin_terceros("B98765431: 7.000,00 € cobrados en metálico", &t),
            "un tercero: 7.000,00 € cobrados en metálico"
        );
        // Sólo palabras enteras, y lo que no nombra a nadie queda igual.
        assert_eq!(sin_terceros("Ref. B98765431X", &t), "Ref. B98765431X");
        let aviso = "La provincia de los perceptores se toma de tu domicilio (sin indicar)";
        assert_eq!(sin_terceros(aviso, &t), aviso);
        assert_eq!(sin_terceros(aviso, &[String::new(), "SL".into()]), aviso);
    }

    #[test]
    fn periodos_y_casillas() {
        // Las casillas de porcentaje no son euros (también en la herramienta `borrador_modelo`).
        assert_eq!(casilla("303", 65, e(10_000)), "[65] 100,00 %");
        assert_eq!(casilla("200", 558, e(2_500)), "[558] 25,00 %");
        assert_eq!(casilla("303", 7, e(470_000)), "[07] 4.700,00 €");
        assert_eq!(periodo("303", 3, 2026), "3T 2026");
        assert_eq!(periodo("202", 2, 2026), "2P 2026");
        assert_eq!(periodo("390", 0, 2025), "2025");
        assert_eq!(del("347", 0, 2025), "de 2025");
        assert_eq!(del("130", 1, 2026), "del 1T 2026");
    }
}
