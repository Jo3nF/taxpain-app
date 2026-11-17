//! IBAN: validación (ISO 13616, módulo 97), la clave de la cuenta y el banco por su código.

use std::sync::LazyLock;

use regex::Regex;

/// Longitud del IBAN por país (los de la zona SEPA y alrededores).
fn longitud(pais: &str) -> Option<usize> {
    Some(match pais {
        "NO" => 15,
        "BE" => 16,
        "DK" | "FI" | "FO" | "GL" | "NL" => 18,
        "MK" | "SI" => 19,
        "AT" | "BA" | "EE" | "KZ" | "LT" | "LU" | "XK" => 20,
        "CH" | "CR" | "HR" | "LI" | "LV" => 21,
        "BG" | "BH" | "DE" | "GB" | "GE" | "IE" | "ME" | "RS" => 22,
        "AE" | "GI" | "IL" | "TL" => 23,
        "AD" | "CZ" | "ES" | "MD" | "PK" | "RO" | "SA" | "SE" | "SK" | "TN" | "VG" => 24,
        "PT" | "ST" => 25,
        "IS" | "TR" => 26,
        "FR" | "GR" | "IT" | "MC" | "MR" | "SM" => 27,
        "AL" | "AZ" | "CY" | "DO" | "GT" | "HU" | "LB" | "PL" => 28,
        "BR" | "PS" | "QA" | "UA" => 29,
        "JO" | "KW" | "MU" => 30,
        "MT" => 31,
        _ => return None,
    })
}

/// IBAN sin espacios ni guiones y en mayúsculas.
pub(crate) fn compactar(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// ¿Es un IBAN válido (país conocido, longitud y dígitos de control)?
pub(crate) fn valido(s: &str) -> bool {
    let t = compactar(s);
    let (Some(pais), Some(control)) = (t.get(..2), t.get(2..4)) else {
        return false;
    };
    if !pais.bytes().all(|b| b.is_ascii_uppercase())
        || !control.bytes().all(|b| b.is_ascii_digit())
        || longitud(pais) != Some(t.len())
    {
        return false;
    }
    // Se pasan los cuatro primeros al final y cada letra a su número (A = 10…): el resto de
    // dividir entre 97 tiene que ser 1. Se va reduciendo para no desbordar.
    let reordenado = t[4..].chars().chain(t[..4].chars());
    let mut resto: u32 = 0;
    for c in reordenado {
        let v = match c.to_digit(36) {
            Some(v) => v,
            None => return false,
        };
        resto = if v < 10 {
            (resto * 10 + v) % 97
        } else {
            (resto * 100 + v) % 97
        };
    }
    resto == 1
}

/// IBAN válidos que aparecen en un texto, sin repetir y en el orden en que aparecen. Admite los
/// escritos en grupos de cuatro («ES91 2100 0418 4502 0005 1332») y seguidos.
pub(crate) fn en_texto(texto: &str) -> Vec<String> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"\b[A-Z]{2}\d{2}(?:[ \u{a0}-]?[A-Z0-9]{4}){2,7}(?:[ \u{a0}-]?[A-Z0-9]{1,3})?")
            .ok()
    });
    let Some(re) = RE.as_ref() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for m in re.find_iter(&texto.to_uppercase()) {
        let t = compactar(m.as_str());
        let Some(n) = t.get(..2).and_then(longitud) else {
            continue;
        };
        // El grupo de detrás puede ser ya otro texto pegado: se prueba con la longitud del país.
        if let Some(iban) = t.get(..n)
            && valido(iban)
            && !out.iter().any(|o| o == iban)
        {
            out.push(iban.to_owned());
        }
    }
    out
}

/// Entidad, oficina y número de cuenta (los diez últimos, sin el dígito de control) de un IBAN
/// español.
pub(crate) fn ccc(iban: &str) -> Option<(String, String, String)> {
    let t = compactar(iban);
    if !t.starts_with("ES") || t.len() != 24 || !t[2..].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((
        t[4..8].to_owned(),
        t[8..12].to_owned(),
        t[14..24].to_owned(),
    ))
}

/// IBAN español de un código cuenta cliente de 20 dígitos (entidad, oficina, control y cuenta;
/// con o sin espacios ni guiones). `None` si no son 20 dígitos.
pub(crate) fn desde_ccc(ccc_: &str) -> Option<String> {
    let d: String = ccc_
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '/' | '.'))
        .collect();
    if d.len() != 20 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Control: 98 − (CCC + «ES00» como números, módulo 97). «E» = 14 y «S» = 28.
    let mut resto: u32 = 0;
    for c in d.chars().chain("142800".chars()) {
        resto = (resto * 10 + c.to_digit(10)?) % 97;
    }
    Some(format!("ES{:02}{d}", 98 - resto))
}

/// Clave estable de la cuenta: con un IBAN español, entidad + oficina + cuenta (la misma que
/// da un extracto Norma 43 de esa cuenta, para que no se duplique); con otro IBAN, el IBAN.
pub(crate) fn clave(iban: &str) -> String {
    match ccc(iban) {
        Some((e, o, c)) => format!("{e}{o}{c}"),
        None => compactar(iban),
    }
}

/// Una cuenta o tarjeta de la que el extracto sólo enseña unos dígitos («ES91...1332»,
/// «ES91 **** **** **** 1332», «****1234», «540000******1234»).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Enmascarada {
    /// País y control, si se ven («ES91»).
    pub prefijo: Option<String>,
    /// Entidad, si se ve (los cuatro dígitos después del control en un IBAN español).
    pub entidad: Option<String>,
    /// Los últimos dígitos que se ven (cuatro, normalmente).
    pub ultimos: String,
}

impl Enmascarada {
    /// Clave provisional de la cuenta, hasta casarla con una cuenta conocida.
    pub(crate) fn clave(&self) -> String {
        format!(
            "{}{}*{}",
            self.prefijo.as_deref().unwrap_or(""),
            self.entidad.as_deref().unwrap_or(""),
            self.ultimos
        )
    }

    /// ¿Puede ser esta cuenta, de la que se conoce el IBAN o la clave completa?
    pub(crate) fn casa_con(&self, iban_o_clave: &str) -> bool {
        let t = compactar(iban_o_clave);
        if !t.ends_with(&self.ultimos) {
            return false;
        }
        if let Some(p) = &self.prefijo
            && t.len() == 24
            && !t.starts_with(p.as_str())
        {
            return false;
        }
        match (&self.entidad, ccc(&t)) {
            (Some(e), Some((ent, _, _))) => *e == ent,
            (Some(e), None) => t.len() == 18 && t.starts_with(e.as_str()),
            (None, _) => true,
        }
    }
}

/// La primera cuenta o tarjeta enmascarada de un texto.
pub(crate) fn enmascarada(texto: &str) -> Option<Enmascarada> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?x)
            (?P<pre>\b[A-Z]{2}\d{2})?[\ \u{a0}]?
            (?P<ent>\d{4})?[\ \u{a0}]?
            (?P<mascara>(?:[*xX•·.…]+[\ \u{a0}]?){1,6})
            (?P<ult>\d{3,4})\b",
        )
        .ok()
    });
    let re = RE.as_ref()?;
    let up = texto.to_uppercase();
    re.captures_iter(&up).find_map(|c| {
        let mascara = c.name("mascara")?.as_str();
        // Al menos tres caracteres de máscara («...», «***», «…» cuenta como tres).
        let n: usize = mascara
            .chars()
            .map(|ch| match ch {
                '…' => 3,
                ' ' | '\u{a0}' => 0,
                _ => 1,
            })
            .sum();
        if n < 3 {
            return None;
        }
        let prefijo = c.name("pre").map(|p| p.as_str().to_owned());
        // La entidad sólo cuenta detrás del país y el control de un IBAN; si no, son los
        // primeros dígitos de una tarjeta, que no identifican el banco.
        let entidad = match (&prefijo, c.name("ent")) {
            (Some(_), Some(e)) => Some(e.as_str().to_owned()),
            _ => None,
        };
        Some(Enmascarada {
            prefijo,
            entidad,
            ultimos: c.name("ult")?.as_str().to_owned(),
        })
    })
}

/// Nombre del banco por el código de entidad de un IBAN español (registro del Banco de
/// España). Sólo las entidades más habituales; las demás se quedan sin nombre.
/// TODO(VERIFY): revisar la lista con el registro de entidades del Banco de España.
pub(crate) fn nombre_entidad(codigo: &str) -> Option<&'static str> {
    Some(match codigo {
        "0019" => "Deutsche Bank",
        "0049" => "Banco Santander",
        "0061" => "Banca March",
        "0073" => "Openbank",
        "0075" => "Banco Popular",
        "0081" => "Banco Sabadell",
        "0128" => "Bankinter",
        "0182" => "BBVA",
        "0186" => "Banco Mediolanum",
        "0239" => "EVO Banco",
        "1465" => "ING",
        "1491" => "Triodos Bank",
        "1583" => "Revolut",
        "2038" => "Bankia",
        "2080" => "Abanca",
        "2085" => "Ibercaja",
        "2095" => "Kutxabank",
        "2100" => "CaixaBank",
        "2103" => "Unicaja",
        "3025" => "Caixa d'Enginyers",
        "3035" => "Laboral Kutxa",
        "3058" => "Cajamar",
        _ => return None,
    })
}

/// Nombre del banco de un IBAN, si se sabe (por ahora, sólo de los españoles).
pub(crate) fn banco(iban: &str) -> Option<&'static str> {
    ccc(iban).and_then(|(e, _, _)| nombre_entidad(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EJEMPLO: &str = "ES9121000418450200051332";

    #[test]
    fn valida_el_iban() {
        assert!(valido(EJEMPLO));
        assert!(valido("es91 2100 0418 4502 0005 1332"));
        assert!(valido("GB82WEST12345698765432"));
        assert!(valido("LT121000011101001000"));
        assert!(!valido("ES9121000418450200051333"));
        assert!(!valido("ES91210004184502000513"));
        assert!(!valido("XX9121000418450200051332"));
    }

    #[test]
    fn encuentra_los_iban_de_un_texto() {
        let t = "Cuenta: ES91 2100 0418 4502 0005 1332 Titular: EMPRESA DEMO SL\n\
                 IBAN GB82WEST12345698765432 y otra vez ES9121000418450200051332.";
        assert_eq!(en_texto(t), [EJEMPLO, "GB82WEST12345698765432"]);
        assert!(en_texto("ES91 2100 0418 4502 0005 1333").is_empty());
    }

    #[test]
    fn clave_compatible_con_norma_43() {
        assert_eq!(
            ccc(EJEMPLO),
            Some((
                "2100".to_owned(),
                "0418".to_owned(),
                "0200051332".to_owned()
            ))
        );
        assert_eq!(clave(EJEMPLO), "210004180200051332");
        assert_eq!(
            clave("GB82 WEST 1234 5698 7654 32"),
            "GB82WEST12345698765432"
        );
        assert_eq!(banco(EJEMPLO), Some("CaixaBank"));
    }

    #[test]
    fn iban_de_un_ccc() {
        assert_eq!(
            desde_ccc("2100 0418 45 0200051332").as_deref(),
            Some(EJEMPLO)
        );
        assert_eq!(desde_ccc("21000418450200051332").as_deref(), Some(EJEMPLO));
        assert_eq!(desde_ccc("2100041845020005133"), None);
    }

    #[test]
    fn cuentas_y_tarjetas_enmascaradas() {
        let m = enmascarada("Cuenta ES91...1332 de EMPRESA DEMO").unwrap();
        assert_eq!(m.prefijo.as_deref(), Some("ES91"));
        assert_eq!(m.ultimos, "1332");
        assert!(m.casa_con(EJEMPLO));
        assert!(!m.casa_con("GB82WEST12345698765432"));

        let m = enmascarada("IBAN ES91 2100 **** **** **** 1332").unwrap();
        assert_eq!(m.entidad.as_deref(), Some("2100"));
        assert!(m.casa_con(EJEMPLO));
        assert!(m.casa_con("210004180200051332"));
        assert!(!m.casa_con("ES0700120345030000067890"));

        let m = enmascarada("Tarjeta 540000******1234").unwrap();
        assert_eq!(
            (m.prefijo, m.entidad, m.ultimos.as_str()),
            (None, None, "1234")
        );
        assert_eq!(
            enmascarada("cuenta …9876").map(|m| m.ultimos),
            Some("9876".to_owned())
        );
        assert_eq!(enmascarada("Saldo 1.234,56"), None);
    }
}
