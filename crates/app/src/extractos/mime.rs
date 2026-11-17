//! PDF que llegan envueltos: guardados desde el correo o desde la web del banco con las
//! cabeceras MIME delante (`MIME-Version`, `Content-Type: multipart/…`), con el PDF tal cual o
//! en base64.

use base64::Engine;

/// Bytes del PDF que hay dentro, si `bytes` no empieza ya por `%PDF` pero lo lleva dentro.
/// `None` si ya es un PDF o si no hay ninguno.
pub(crate) fn pdf_dentro(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.starts_with(b"%PDF") {
        return None;
    }
    // El PDF tal cual dentro del sobre: desde `%PDF-` hasta el último `%%EOF`.
    if let Some(ini) = buscar(bytes, b"%PDF-") {
        let fin = buscar_ultimo(bytes, b"%%EOF").map_or(bytes.len(), |p| p + 5);
        if fin > ini {
            return Some(bytes[ini..fin].to_vec());
        }
    }
    // En base64: la parte cuyo tipo es PDF, hasta la siguiente frontera.
    let texto = String::from_utf8_lossy(bytes);
    let lower = texto.to_ascii_lowercase();
    let mut desde = 0;
    while let Some(p) = lower[desde..].find("application/pdf") {
        let cabecera = desde + p;
        desde = cabecera + 1;
        let Some(cuerpo) = lower[cabecera..]
            .find("\r\n\r\n")
            .map(|q| cabecera + q + 4)
            .or_else(|| lower[cabecera..].find("\n\n").map(|q| cabecera + q + 2))
        else {
            continue;
        };
        if !lower[cabecera..cuerpo].contains("base64") {
            continue;
        }
        let fin = texto[cuerpo..]
            .find("\n--")
            .map_or(texto.len(), |q| cuerpo + q);
        let limpio: String = texto[cuerpo..fin]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if let Ok(pdf) = base64::engine::general_purpose::STANDARD.decode(limpio.as_bytes())
            && pdf.starts_with(b"%PDF")
        {
            return Some(pdf);
        }
    }
    None
}

fn buscar(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn buscar_ultimo(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).rposition(|w| w == n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saca_el_pdf_del_sobre() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n";
        let mut sobre = b"MIME-Version: 1.0\r\nContent-Type: multipart/related; \
boundary=\"----=_Part_1\"\r\n\r\n------=_Part_1\r\nContent-Type: application/pdf\r\n\r\n"
            .to_vec();
        sobre.extend_from_slice(pdf);
        sobre.extend_from_slice(b"\r\n------=_Part_1--\r\n");
        assert_eq!(pdf_dentro(&sobre).as_deref(), Some(&pdf[..pdf.len() - 1]));

        let b64 = base64::engine::general_purpose::STANDARD.encode(pdf);
        let sobre = format!(
            "MIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"\n\n--b\n\
             Content-Type: application/pdf; name=\"extracto.pdf\"\n\
             Content-Transfer-Encoding: base64\n\n{}\n{}\n--b--\n",
            &b64[..20],
            &b64[20..]
        );
        assert_eq!(pdf_dentro(sobre.as_bytes()).as_deref(), Some(&pdf[..]));

        assert_eq!(pdf_dentro(pdf), None);
        assert_eq!(pdf_dentro(b"hola"), None);
    }
}
