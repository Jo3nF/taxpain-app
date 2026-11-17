//! Número de Identificación Fiscal: DNI, NIE, NIF de personas físicas especiales (K, L, M) y
//! NIF de personas jurídicas y entidades.
//!
//! Normas: composición del NIF en el RD 1065/2007, arts. 18–22, y la Orden EHA/451/2008
//! (claves de la letra inicial de entidades). Letra de control del DNI/NIE: módulo 23
//! (Ministerio del Interior). Dígito de control de entidades: algoritmo de la Orden EHA/451/2008
//! (suma de posiciones pares + dígitos de los dobles de las impares).
//! `TODO(VERIFY)`: qué letras iniciales exigen control letra/número (P, Q, R, S, N, W → letra;
//! A, B, E, H → número) contra el texto consolidado de la Orden EHA/451/2008.

use std::fmt;

const DNI_LETTERS: &[u8; 23] = b"TRWAGMYFPDXBNJZSQVHLCKE";
const ENTITY_CONTROL_LETTERS: &[u8; 10] = b"JABCDEFGHI";
/// Letras iniciales válidas de NIF de entidades (Orden EHA/451/2008).
const ENTITY_LETTERS: &str = "ABCDEFGHJNPQRSUVW";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NifKind {
    /// Españoles con DNI: 8 cifras + letra.
    Dni,
    /// Extranjeros con NIE: X/Y/Z + 7 cifras + letra.
    Nie,
    /// Personas físicas sin DNI ni NIE (K menores de 14, L residentes en el extranjero,
    /// M extranjeros sin NIE).
    PersonaFisicaEspecial,
    /// Personas jurídicas y entidades sin personalidad (letra inicial = forma jurídica).
    Entidad,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NifError {
    #[error("el NIF debe tener 9 caracteres")]
    Length,
    #[error("formato de NIF no reconocido")]
    Format,
    #[error("el carácter de control del NIF no es correcto")]
    Control,
}

/// NIF validado y normalizado (mayúsculas, sin espacios, puntos ni guiones).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Nif {
    value: String,
    kind: NifKind,
}

impl Nif {
    pub fn parse(input: &str) -> Result<Nif, NifError> {
        let value: String = input
            .chars()
            .filter(|c| !matches!(c, ' ' | '.' | '-'))
            .flat_map(char::to_uppercase)
            .collect();
        if value.len() != 9 || !value.is_ascii() {
            return Err(NifError::Length);
        }
        let b = value.as_bytes();
        let body_digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);

        let kind = match b[0] {
            b'0'..=b'9' => {
                if !body_digits(0..8) {
                    return Err(NifError::Format);
                }
                check_dni_letter(&value[0..8], b[8])?;
                NifKind::Dni
            }
            b'X' | b'Y' | b'Z' => {
                if !body_digits(1..8) {
                    return Err(NifError::Format);
                }
                let prefix = b[0] - b'X'; // X→0, Y→1, Z→2
                let digits = format!("{prefix}{}", &value[1..8]);
                check_dni_letter(&digits, b[8])?;
                NifKind::Nie
            }
            b'K' | b'L' | b'M' => {
                if !body_digits(1..8) {
                    return Err(NifError::Format);
                }
                check_dni_letter(&value[1..8], b[8])?;
                NifKind::PersonaFisicaEspecial
            }
            c if ENTITY_LETTERS.as_bytes().contains(&c) => {
                if !body_digits(1..8) {
                    return Err(NifError::Format);
                }
                check_entity_control(c, &b[1..8], b[8])?;
                NifKind::Entidad
            }
            _ => return Err(NifError::Format),
        };
        Ok(Nif { value, kind })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn kind(&self) -> NifKind {
        self.kind
    }

    pub fn is_persona_fisica(&self) -> bool {
        !matches!(self.kind, NifKind::Entidad)
    }
}

impl fmt::Display for Nif {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

fn check_dni_letter(digits: &str, letter: u8) -> Result<(), NifError> {
    let n: u32 = digits.parse().map_err(|_| NifError::Format)?;
    if DNI_LETTERS[(n % 23) as usize] == letter {
        Ok(())
    } else {
        Err(NifError::Control)
    }
}

/// Dígito de control de entidades: suma de las cifras en posición par (2.ª, 4.ª, 6.ª) más la
/// suma de los dígitos del doble de las cifras en posición impar; control = (10 − suma mod 10)
/// mod 10, expresado como cifra o como letra `JABCDEFGHI[control]`.
fn check_entity_control(first: u8, digits: &[u8], control: u8) -> Result<(), NifError> {
    let mut sum = 0u32;
    for (i, d) in digits.iter().enumerate() {
        let d = u32::from(d - b'0');
        if i % 2 == 0 {
            let doubled = d * 2;
            sum += doubled / 10 + doubled % 10;
        } else {
            sum += d;
        }
    }
    let c = ((10 - sum % 10) % 10) as usize;
    let digit = b'0' + c as u8;
    let letter = ENTITY_CONTROL_LETTERS[c];
    let ok = match first {
        b'P' | b'Q' | b'R' | b'S' | b'N' | b'W' => control == letter,
        b'A' | b'B' | b'E' | b'H' => control == digit,
        _ => control == digit || control == letter,
    };
    if ok { Ok(()) } else { Err(NifError::Control) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dni() {
        let nif = Nif::parse("12345678z").unwrap();
        assert_eq!(nif.as_str(), "12345678Z");
        assert_eq!(nif.kind(), NifKind::Dni);
        assert_eq!(Nif::parse("12.345.678-Z").unwrap().as_str(), "12345678Z");
        assert_eq!(Nif::parse("12345678A"), Err(NifError::Control));
        assert_eq!(Nif::parse("00000000T").unwrap().kind(), NifKind::Dni);
    }

    #[test]
    fn nie() {
        // X1234567L: 01234567 mod 23 = 19 → L
        assert_eq!(Nif::parse("X1234567L").unwrap().kind(), NifKind::Nie);
        // Y1234567X: 11234567 mod 23 = 10 → X
        assert_eq!(Nif::parse("Y1234567X").unwrap().kind(), NifKind::Nie);
        assert_eq!(Nif::parse("X1234567A"), Err(NifError::Control));
    }

    #[test]
    fn entidades() {
        assert_eq!(Nif::parse("B12345674").unwrap().kind(), NifKind::Entidad);
        assert_eq!(Nif::parse("A22222228").unwrap().kind(), NifKind::Entidad);
        assert_eq!(Nif::parse("B12345675"), Err(NifError::Control));
        // Organismo público (control letra): Q2826000H
        assert_eq!(Nif::parse("Q2826000H").unwrap().kind(), NifKind::Entidad);
        assert_eq!(Nif::parse("Q28260008"), Err(NifError::Control));
        assert!(!Nif::parse("B12345674").unwrap().is_persona_fisica());
    }

    #[test]
    fn formato() {
        assert_eq!(Nif::parse("1234"), Err(NifError::Length));
        assert_eq!(Nif::parse("I12345674"), Err(NifError::Format));
        assert_eq!(Nif::parse("B12A45674"), Err(NifError::Format));
        assert_eq!(Nif::parse("B1234567X"), Err(NifError::Control));
        assert_eq!(Nif::parse("ñ12345678"), Err(NifError::Length));
    }
}
