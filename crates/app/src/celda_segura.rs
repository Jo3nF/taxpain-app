//! Texto de los datos en hojas de cálculo, sin que se convierta en fórmula.
//!
//! Un concepto, un nombre o una descripción que empiece por `=`, `+`, `-`, `@`, tabulador o
//! retorno de carro puede acabar ejecutándose como fórmula al abrir o editar la hoja en Excel,
//! LibreOffice o Numbers («inyección de fórmulas», OWASP *CSV Injection*). Aquí está la regla
//! común para todas las exportaciones:
//!
//! - XLSX: siempre `write_string` (nunca `write_formula` ni `write`, que adivina el tipo) y, si
//!   el texto es peligroso, con el prefijo de comilla del formato (`quotePrefix`), que hace que
//!   la celda siga siendo texto aunque se edite.
//! - CSV: se antepone una comilla simple al campo peligroso.
//!
//! Los importes se escriben como número aparte (`write_number`): un importe negativo sigue
//! siendo un número, no pasa por aquí.

use std::borrow::Cow;

use rust_xlsxwriter::{Format, Worksheet, XlsxError};

/// Primeros caracteres con los que una hoja de cálculo puede leer una fórmula.
const PELIGROSOS: [char; 7] = ['=', '+', '-', '@', '\t', '\r', '\n'];

/// Límite de Excel por celda (32.767 caracteres, contados en UTF-16).
const MAX_CELDA: usize = 32_767;
const RECORTE: &str = " […] (texto recortado: no cabe entero en una celda de Excel)";

/// El texto, recortado si no cabe en una celda (en un límite de carácter).
fn cabe(s: &str) -> Cow<'_, str> {
    if s.encode_utf16().count() <= MAX_CELDA {
        return Cow::Borrowed(s);
    }
    let max = MAX_CELDA - RECORTE.encode_utf16().count();
    let mut n = 0;
    let mut fin = 0;
    for (i, c) in s.char_indices() {
        n += c.len_utf16();
        if n > max {
            break;
        }
        fin = i + c.len_utf8();
    }
    Cow::Owned(format!("{}{RECORTE}", &s[..fin]))
}

/// `true` si el texto, escrito tal cual, podría tomarse por una fórmula.
#[must_use]
pub fn es_peligroso(s: &str) -> bool {
    s.starts_with(PELIGROSOS)
}

/// Campo de CSV a salvo: con una comilla simple delante si podría tomarse por fórmula.
#[must_use]
#[allow(dead_code)] // Aún no hay ninguna exportación CSV: queda lista para la primera.
pub fn csv(s: &str) -> Cow<'_, str> {
    if es_peligroso(s) {
        Cow::Owned(format!("'{s}"))
    } else {
        Cow::Borrowed(s)
    }
}

/// Escribe texto de datos en una celda XLSX, siempre como texto.
///
/// # Errors
/// Los de `rust_xlsxwriter` (fila o columna fuera de rango, texto demasiado largo).
pub fn texto(ws: &mut Worksheet, row: u32, col: u16, s: impl AsRef<str>) -> Result<(), XlsxError> {
    let s = cabe(s.as_ref());
    let s = s.as_ref();
    if es_peligroso(s) {
        ws.write_string_with_format(row, col, s, &Format::new().set_quote_prefix())?;
    } else {
        ws.write_string(row, col, s)?;
    }
    Ok(())
}

/// Como [`texto`], con un formato (negrita…).
///
/// # Errors
/// Los de `rust_xlsxwriter`.
pub fn texto_con_formato(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    s: impl AsRef<str>,
    format: &Format,
) -> Result<(), XlsxError> {
    let s = cabe(s.as_ref());
    let s = s.as_ref();
    if es_peligroso(s) {
        ws.write_string_with_format(row, col, s, &format.clone().set_quote_prefix())?;
    } else {
        ws.write_string_with_format(row, col, s, format)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use rust_xlsxwriter::Workbook;

    use super::*;

    #[test]
    fn un_texto_demasiado_largo_se_recorta_y_se_puede_escribir() {
        let largo = "ñ".repeat(40_000);
        let r = cabe(&largo);
        assert!(r.encode_utf16().count() <= MAX_CELDA);
        assert!(r.ends_with(RECORTE));
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        texto(ws, 0, 0, &largo).unwrap();
        wb.save_to_buffer().unwrap();
        assert_eq!(cabe("corto"), "corto");
    }

    #[test]
    fn detecta_los_primeros_caracteres_peligrosos() {
        for s in [
            "=1+1",
            "+34 600",
            "-2+3",
            "@SUM(A1)",
            "\t=1",
            "\r=1",
            "=HYPERLINK(\"http://x\")",
        ] {
            assert!(es_peligroso(s), "{s:?}");
        }
        for s in ["", "Alquiler", "1.234,56", "B12345674", " =1", "Pago 50-50"] {
            assert!(!es_peligroso(s), "{s:?}");
        }
    }

    #[test]
    fn csv_antepone_una_comilla() {
        assert_eq!(csv("=1+1"), "'=1+1");
        assert_eq!(csv("@x"), "'@x");
        assert_eq!(csv("Alquiler"), "Alquiler");
        assert!(matches!(csv("Alquiler"), Cow::Borrowed(_)));
    }

    fn parte(xlsx: &[u8], nombre: &str) -> String {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(xlsx)).unwrap();
        let mut s = String::new();
        z.by_name(nombre).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn xlsx_escribe_texto_con_prefijo_y_los_numeros_negativos_siguen_siendo_numeros() {
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        texto(ws, 0, 0, "=HYPERLINK(\"http://x\",\"pincha\")").unwrap();
        texto_con_formato(ws, 0, 1, "+1", &Format::new().set_bold()).unwrap();
        texto(ws, 0, 2, "Alquiler").unwrap();
        ws.write_number(0, 3, -12.5).unwrap();
        let bytes = wb.save_to_buffer().unwrap();
        let hoja = parte(&bytes, "xl/worksheets/sheet1.xml");
        assert!(!hoja.contains("<f>"), "{hoja}");
        assert!(hoja.contains(r#"<c r="A1" s="1" t="s">"#), "{hoja}");
        assert!(hoja.contains(r#"<c r="C1" t="s">"#), "{hoja}");
        assert!(hoja.contains("<v>-12.5</v>"), "{hoja}");
        let estilos = parte(&bytes, "xl/styles.xml");
        assert!(estilos.contains(r#"quotePrefix="1""#), "{estilos}");
    }

    /// `codigo` sin los elementos marcados con `#[cfg(test)]` (estén donde estén, no sólo al
    /// final), con sus llaves emparejadas. Las llaves de cadenas, caracteres y comentarios no
    /// cuentan; las cadenas crudas con `#` se tratan como cadenas normales, que es suficiente
    /// para el código de este repositorio.
    fn sin_pruebas(codigo: &str) -> String {
        let b = codigo.as_bytes();
        let mut out = String::with_capacity(codigo.len());
        let mut desde = 0;
        while let Some(p) = codigo[desde..].find("#[cfg(test)]") {
            let p = desde + p;
            out.push_str(&codigo[desde..p]);
            // El elemento acaba en el `;` o en la `}` que cierra su primera `{`.
            let mut i = p + "#[cfg(test)]".len();
            let mut nivel = 0usize;
            while i < b.len() {
                match b[i] {
                    b'"' => {
                        i += 1;
                        while i < b.len() && b[i] != b'"' {
                            i += if b[i] == b'\\' { 2 } else { 1 };
                        }
                    }
                    b'\'' if b.get(i + 2) == Some(&b'\'') => i += 2,
                    b'\'' if b.get(i + 1) == Some(&b'\\') && b.get(i + 3) == Some(&b'\'') => i += 3,
                    b'/' if b.get(i + 1) == Some(&b'/') => {
                        while i < b.len() && b[i] != b'\n' {
                            i += 1;
                        }
                    }
                    b'{' => nivel += 1,
                    b'}' => {
                        nivel = nivel.saturating_sub(1);
                        if nivel == 0 {
                            break;
                        }
                    }
                    b';' if nivel == 0 => break,
                    _ => {}
                }
                i += 1;
            }
            // Se conservan los saltos de línea, para que los números de línea sigan valiendo.
            let fin = (i + 1).min(b.len());
            out.extend(codigo[p..fin].chars().filter(|&c| c == '\n'));
            desde = fin;
        }
        out.push_str(&codigo[desde..]);
        out
    }

    #[test]
    fn sin_pruebas_quita_todos_los_modulos_de_prueba() {
        let codigo = "a\n#[cfg(test)]\nmod t {\n fn x() { let s = \"}\"; let c = '}'; } // }\n}\nb\n\
                      #[cfg(test)]\nmod u;\nc\n#[cfg(test)]\nfn y() {}\nd";
        let limpio = sin_pruebas(codigo);
        assert_eq!(limpio.replace('\n', ""), "abcd", "{limpio:?}");
        assert_eq!(limpio.lines().count(), codigo.lines().count());
    }

    /// Ninguna hoja de cálculo se escribe sin pasar por aquí: fuera de este fichero y del código
    /// de pruebas (los elementos `#[cfg(test)]`, estén donde estén, y las carpetas `tests/`,
    /// que crean hojas de entrada), ningún `.rs` de `crates/` que nombre `rust_xlsxwriter` o
    /// `Worksheet` (también si se reexporta con otro nombre: el `use` lo nombra) llama a los
    /// `write*` de `Worksheet` que escriben texto o fórmulas.
    #[test]
    fn nadie_escribe_texto_en_una_hoja_sin_pasar_por_aqui() {
        let prohibido = regex::Regex::new(
            r"\.write(_string|_formula|_array_formula|_dynamic_formula|_dynamic_array_formula|_rich_string|_row|_column|_row_matrix|_column_matrix)?(_with_format)?\(",
        )
        .unwrap();
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let mut pendientes = vec![crates.to_path_buf()];
        let mut mirados = 0;
        let mut malos = Vec::new();
        while let Some(dir) = pendientes.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let p = e.unwrap().path();
                let nombre = p.file_name().unwrap().to_string_lossy().into_owned();
                if p.is_dir() {
                    if nombre != "target" && nombre != "tests" && !nombre.starts_with('.') {
                        pendientes.push(p);
                    }
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") || nombre == "celda_segura.rs" {
                    continue;
                }
                let codigo = std::fs::read_to_string(&p).unwrap();
                if !codigo.contains("rust_xlsxwriter") && !codigo.contains("Worksheet") {
                    continue;
                }
                mirados += 1;
                for (n, linea) in sin_pruebas(&codigo).lines().enumerate() {
                    if prohibido.is_match(linea) && !linea.contains(".write(true)") {
                        malos.push(format!("{}:{}: {}", p.display(), n + 1, linea.trim()));
                    }
                }
            }
        }
        assert!(
            mirados >= 5,
            "no se han encontrado las exportaciones ({mirados})"
        );
        assert!(
            malos.is_empty(),
            "usa crate::celda_segura:\n{}",
            malos.join("\n")
        );
    }

    #[test]
    fn las_celdas_de_los_libros_pasan_por_la_regla() {
        use taxpain_core::money::Money;
        use taxpain_invoicing::books::Cell;
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        let dos = Format::new().set_num_format("0.00");
        crate::books::write_cell(ws, 0, 0, &Cell::Text("=1+1".into()), &dos).unwrap();
        crate::books::write_cell(ws, 0, 1, &Cell::Amount(Money::from_cents(-1050)), &dos).unwrap();
        let bytes = wb.save_to_buffer().unwrap();
        let hoja = parte(&bytes, "xl/worksheets/sheet1.xml");
        assert!(!hoja.contains("<f>"), "{hoja}");
        assert!(hoja.contains("<v>-10.5</v>"), "{hoja}");
        assert!(parte(&bytes, "xl/styles.xml").contains(r#"quotePrefix="1""#));
    }
}
