//! Países: códigos ISO 3166-1 alfa-2, los que admiten los modelos de la AEAT en el campo
//! «código de país» (diseños de registro del 349, 190, 347…; VIES para el NIF-IVA) y los
//! libros registro (columna «Código País» del libro de facturas, Orden HAC/1177/2024).

/// Los 249 códigos ISO 3166-1 alfa-2 asignados (edición vigente), en orden alfabético.
pub const COUNTRIES: &[&str] = &[
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX", "AZ",
    "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR", "BS",
    "BT", "BV", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM", "CN",
    "CO", "CR", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ", "EC", "EE",
    "EG", "EH", "ER", "ES", "ET", "FI", "FJ", "FK", "FM", "FO", "FR", "GA", "GB", "GD", "GE", "GF",
    "GG", "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT", "GU", "GW", "GY", "HK", "HM",
    "HN", "HR", "HT", "HU", "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT", "JE", "JM",
    "JO", "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB", "LC",
    "LI", "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK",
    "ML", "MM", "MN", "MO", "MP", "MQ", "MR", "MS", "MT", "MU", "MV", "MW", "MX", "MY", "MZ", "NA",
    "NC", "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "PA", "PE", "PF", "PG",
    "PH", "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS", "RU", "RW",
    "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO", "SR", "SS",
    "ST", "SV", "SX", "SY", "SZ", "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO",
    "TR", "TT", "TV", "TW", "TZ", "UA", "UG", "UM", "US", "UY", "UZ", "VA", "VC", "VE", "VG", "VI",
    "VN", "VU", "WF", "WS", "YE", "YT", "ZA", "ZM", "ZW",
];

/// `code` (en cualquier caja) es un código de país ISO 3166-1 alfa-2 asignado. «EL» (Grecia
/// en VIES) y «XI» (Irlanda del Norte) no son ISO: ver [`crate::eu`].
pub fn is_country(code: &str) -> bool {
    let c = code.trim().to_ascii_uppercase();
    COUNTRIES.binary_search(&c.as_str()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_lista_es_la_iso_completa_y_ordenada() {
        assert_eq!(COUNTRIES.len(), 249);
        assert!(COUNTRIES.windows(2).all(|w| w[0] < w[1]));
        assert!(COUNTRIES.iter().all(|c| c.len() == 2));
        for c in crate::eu::EU_COUNTRIES {
            assert!(is_country(c) || matches!(*c, "EL" | "XI"), "{c}");
        }
        assert!(is_country("us") && is_country("ES") && is_country("AX"));
        assert!(!is_country("ZZ") && !is_country("EL") && !is_country("XI") && !is_country(""));
    }
}
