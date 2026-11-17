//! Número de periodo de las autoliquidaciones periódicas (303, 111, 115, 123, 349).
//!
//! Los cálculos y los ficheros llevan el periodo en un `u8` (el campo `quarter`): del 1 al 4 es
//! el trimestre; del 101 al 112, el mes (101 = enero). Así los mensuales (gran empresa, REDEME,
//! SII: arts. 71.3 RIVA y 108.1 RIRPF; 349 mensual, art. 81.2 RIVA) usan los mismos cálculos,
//! que ya van por fechas.
//!
//! Los diseños de registro del 303, del 111, del 115 y del 123 admiten el periodo «01»…«12»
//! además de «1T»…«4T» (campo «Periodo»: `fixtures/aeat/dr/{303,111,115,123}.json`), y el 349,
//! el periodo de dos caracteres (`349.json`, registro de declarante, campo 13).

/// Primer número de mes menos uno: enero es `MES + 1`.
pub const MES: u8 = 100;

/// Periodo de un mes (1–12).
pub const fn de_mes(m: u8) -> u8 {
    MES + m
}

/// El mes (1–12), si el periodo es mensual.
pub fn mes(p: u8) -> Option<u8> {
    (MES + 1..=MES + 12).contains(&p).then(|| p - MES)
}

/// ¿Es un periodo válido (trimestre o mes)?
pub fn valido(p: u8) -> bool {
    (1..=4).contains(&p) || mes(p).is_some()
}

/// Trimestre al que pertenece el periodo (el propio trimestre o el del mes).
pub fn trimestre(p: u8) -> u8 {
    mes(p).map_or(p, |m| m.div_ceil(3))
}

/// Código del periodo en el fichero de la AEAT: «3T» o «07».
pub fn codigo(p: u8) -> String {
    match mes(p) {
        Some(m) => format!("{m:02}"),
        None => format!("{p}T"),
    }
}

/// El periodo de un código («3T», «07», «7»). `None` si no es trimestre ni mes.
pub fn de_codigo(c: &str) -> Option<u8> {
    let c = c.trim().to_ascii_uppercase();
    if let Some(q) = c.strip_suffix('T') {
        return q.parse::<u8>().ok().filter(|q| (1..=4).contains(q));
    }
    c.parse::<u8>()
        .ok()
        .filter(|m| (1..=12).contains(m))
        .map(de_mes)
}

/// ¿Es el último periodo del año (4T o diciembre)? En él van la regularización de la prorrata,
/// la devolución del saldo a favor y el apartado de los exonerados del 390.
pub fn ultimo(p: u8) -> bool {
    p == 4 || p == de_mes(12)
}

/// El periodo anterior del mismo año y la misma periodicidad (`None` en el primero).
pub fn anterior(p: u8) -> Option<u8> {
    match mes(p) {
        Some(1) => None,
        Some(_) => Some(p - 1),
        None => (p > 1).then(|| p - 1),
    }
}

/// Primer periodo del año con la periodicidad de `p`.
pub fn primero(p: u8) -> u8 {
    if mes(p).is_some() { de_mes(1) } else { 1 }
}

/// Último periodo del año con la periodicidad de `p`.
pub fn ultimo_de(p: u8) -> u8 {
    if mes(p).is_some() { de_mes(12) } else { 4 }
}

/// «3T 2026» o «julio de 2026».
pub fn nombre(p: u8, year: i32) -> String {
    const MESES: [&str; 12] = [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ];
    match mes(p) {
        Some(m) => format!("{} de {year}", MESES[usize::from(m - 1)]),
        None => format!("{p}T {year}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimestres_y_meses() {
        assert_eq!(codigo(3), "3T");
        assert_eq!(codigo(de_mes(7)), "07");
        assert_eq!(de_codigo("07"), Some(107));
        assert_eq!(de_codigo("4t"), Some(4));
        assert_eq!(de_codigo("13"), None);
        assert_eq!(de_codigo("0A"), None);
        assert_eq!(trimestre(de_mes(7)), 3);
        assert_eq!(trimestre(2), 2);
        assert!(ultimo(4) && ultimo(112) && !ultimo(111) && !ultimo(3));
        assert_eq!(anterior(101), None);
        assert_eq!(anterior(105), Some(104));
        assert_eq!(anterior(1), None);
        assert!(valido(112) && !valido(113) && !valido(0) && !valido(5));
        assert_eq!(nombre(103, 2026), "marzo de 2026");
    }
}
