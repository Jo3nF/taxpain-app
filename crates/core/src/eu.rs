//! Estados miembros de la Unión Europea, para las operaciones intracomunitarias del IVA
//! (arts. 13 a 16, 25 y 69 LIVA; modelo 349 y casilla [59] del 303).

/// Estados miembros de la UE con los prefijos del NIF-IVA (VIES): Grecia usa «EL» y «XI» es
/// Irlanda del Norte, que sólo cuenta como Estado miembro para los **bienes** (Protocolo sobre
/// Irlanda del Norte / Marco de Windsor); para los servicios es Reino Unido.
pub const EU_COUNTRIES: &[&str] = &[
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "EL", "ES", "FI", "FR", "HR", "HU", "IE", "IT",
    "LT", "LU", "LV", "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK", "XI",
];

/// Prefijo VIES de un código de país: mayúsculas y Grecia «GR» → «EL».
pub fn vies_prefix(country: &str) -> String {
    match country.trim().to_ascii_uppercase().as_str() {
        "GR" => "EL".to_owned(),
        c => c.to_owned(),
    }
}

/// `country` es otro Estado miembro de la UE (no España) para una operación intracomunitaria:
/// de bienes (`goods`) o de servicios. Irlanda del Norte (XI) sólo vale para bienes.
pub fn is_other_member_state(country: &str, goods: bool) -> bool {
    let c = vies_prefix(country);
    c != "ES" && (goods || c != "XI") && EU_COUNTRIES.contains(&c.as_str())
}

/// Estructura del NIF-IVA de cada Estado miembro tras el prefijo: longitudes admitidas y si es
/// sólo numérico (si no, cifras y mayúsculas). Tabla «Estructura NIF-IVA» de la nota (1) de las
/// Validaciones VERI*FACTU v1.2.2 §3.1.3.13 (la de VIES): la AEAT rechaza el registro de
/// facturación, y el 349, si el identificador no se ajusta a ella.
const VAT_ID_SHAPES: &[(&str, &[usize], bool)] = &[
    ("AT", &[9], false), // U + 8 cifras
    ("BE", &[10], true),
    ("BG", &[9, 10], true),
    ("CY", &[9], false), // 8 cifras + letra
    ("CZ", &[8, 9, 10], true),
    ("DE", &[9], true),
    ("DK", &[8], true),
    ("EE", &[9], true),
    ("EL", &[9], true),
    ("FI", &[8], true),
    ("FR", &[11], false), // 2 alfanuméricos + 9 cifras
    ("HR", &[11], true),
    ("HU", &[8], true),
    ("IE", &[8, 9], false),
    ("IT", &[11], true),
    ("LT", &[9, 12], true),
    ("LU", &[8], true),
    ("LV", &[11], true),
    ("MT", &[8], true),
    ("NL", &[12], false), // 9 alfanuméricos + B + 2 cifras
    ("PL", &[10], true),
    ("PT", &[9], true),
    ("RO", &[2, 3, 4, 5, 6, 7, 8, 9, 10], true),
    ("SE", &[12], true),
    ("SI", &[8], true),
    ("SK", &[10], true),
    ("XI", &[5, 9, 12], false),
];

/// Comprueba la estructura de un NIF-IVA completo («FR12345678901»): prefijo de otro Estado
/// miembro y, tras él, la longitud y la clase de caracteres de ese Estado (sólo mayúsculas).
pub fn check_vat_id_structure(vat_id: &str) -> Result<(), String> {
    if !vat_id.is_char_boundary(2) || vat_id.len() < 2 {
        return Err("falta el NIF-IVA (prefijo del país y número)".into());
    }
    let (prefix, body) = vat_id.split_at(2);
    let Some((_, lens, digits)) = VAT_ID_SHAPES.iter().find(|(p, _, _)| *p == prefix) else {
        return Err(format!(
            "«{prefix}» no es el prefijo del NIF-IVA de otro Estado miembro de la UE"
        ));
    };
    let ok_chars = body.chars().all(|c| {
        if *digits {
            c.is_ascii_digit()
        } else {
            c.is_ascii_digit() || c.is_ascii_uppercase()
        }
    });
    if lens.contains(&body.len()) && ok_chars {
        return Ok(());
    }
    let lens_txt = match (lens.first(), lens.last()) {
        (Some(a), Some(b)) if lens.len() > 3 => format!("de {a} a {b}"),
        _ => lens
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" u "),
    };
    Err(format!(
        "el NIF-IVA de {prefix} lleva tras «{prefix}» {lens_txt} caracteres{}: «{body}» no encaja \
         (compruébalo en VIES)",
        if *digits {
            ", sólo cifras"
        } else {
            " (cifras y mayúsculas)"
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estados_miembros() {
        assert!(is_other_member_state("FR", false));
        assert!(is_other_member_state("gr", true));
        assert!(is_other_member_state("EL", false));
        assert!(!is_other_member_state("ES", true));
        assert!(!is_other_member_state("US", false));
        assert!(!is_other_member_state("GB", true));
        assert!(!is_other_member_state("CH", true));
        // Irlanda del Norte: bienes sí, servicios no.
        assert!(is_other_member_state("XI", true));
        assert!(!is_other_member_state("XI", false));
    }

    #[test]
    fn estructura_del_nif_iva() {
        for ok in [
            "DE123456789",
            "FR12345678901",
            "FRAB123456789",
            "IT12345678901",
            "PT123456789",
            "NL123456789B01",
            "IE1234567X",
            "IE1X23456X",
            "EL123456789",
            "XI123456789",
            "XIGD123",
            "ATU12345678",
            "RO12",
            "BE0123456789",
        ] {
            assert_eq!(check_vat_id_structure(ok), Ok(()), "{ok}");
        }
        // Cortos, con letras donde van cifras, en minúsculas o de fuera de la UE.
        for ko in [
            "FR1",
            "DE12",
            "DE12345678A",
            "de123456789",
            "FR1234567890",
            "US123456789",
            "ES12345678Z",
            "F",
            "",
        ] {
            assert!(check_vat_id_structure(ko).is_err(), "{ko}");
        }
        let e = check_vat_id_structure("FR1").unwrap_err();
        assert!(e.contains("11 caracteres") && e.contains("VIES"), "{e}");
        let e = check_vat_id_structure("RO1").unwrap_err();
        assert!(e.contains("de 2 a 10"), "{e}");
    }
}
