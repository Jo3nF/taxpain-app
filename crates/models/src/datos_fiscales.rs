//! Lectura del fichero de datos fiscales de Renta (modelo 100) para cotejar la actividad.
//!
//! Diseño: «Diseño de registro de datos fiscales y personales IRPF 2025», v1.3
//! (`fixtures/aeat/renta/`): registros de posiciones fijas; importes en céntimos con «N» inicial
//! si son negativos. Sólo se interpretan los registros útiles para una actividad económica
//! (RTC, PF, CT, RET); el resto se cuenta y queda en el fichero guardado.
//! `TODO(VERIFY)`: separador de registros (se aceptan `\n` y `\r\n`; el diseño no lo dice).

use serde::{Deserialize, Serialize};
use taxpain_core::Money;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaxDataError {
    #[error(
        "tu domicilio fiscal no está ratificado para la Renta: ratifícalo en la Sede antes de descargar los datos fiscales"
    )]
    NotRatified,
    #[error("la AEAT respondió: {0}")]
    Service(String),
    #[error("el fichero de datos fiscales no tiene el formato esperado: {0}")]
    Format(String),
}

/// Rendimiento de actividad económica comunicado por un pagador en su modelo 190 (registro RTC).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityIncome {
    pub payer_nif: String,
    pub payer_name: String,
    /// Clave y subclave del 190 (G1…G8 profesionales, H1…H4 estimación objetiva, I1…I3, J).
    pub key: String,
    pub income: Money,
    pub withholding: Money,
}

/// Pago fraccionado (registro PF, modelos 130 y 131).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installment {
    pub model: String,
    /// Tipo de declaración (I ingreso, N negativa, C compensación, D devolución…).
    pub kind: String,
    pub period: String,
    pub receipt: String,
    pub amount: Money,
}

/// Cotización a la Seguridad Social (registro CT).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contribution {
    /// Régimen: 0521 autónomos, 0825 autónomos del mar…
    pub regime: String,
    pub amount: Money,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxData {
    /// Ejercicio del código de cabecera (`DDFF2025`).
    pub year: i32,
    /// Fecha de proceso `AAAAMMDD`.
    pub process_date: Option<String>,
    pub activities: Vec<ActivityIncome>,
    pub installments: Vec<Installment>,
    pub contributions: Vec<Contribution>,
    /// Regularización del RETA: (a ingresar, a devolver).
    pub reta_regularization: Vec<(Money, Money)>,
    /// Registros de otros tipos (no se interpretan; quedan en el fichero guardado).
    pub other_records: usize,
}

/// Campo de posiciones 1-based `[pos, pos+len)` (por caracteres).
fn field(chars: &[char], pos: usize, len: usize) -> String {
    chars
        .get(pos - 1..(pos - 1 + len).min(chars.len()))
        .map(|s| s.iter().collect::<String>())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Importe en céntimos; «N» en la primera posición = negativo.
fn amount(chars: &[char], pos: usize, len: usize) -> Result<Money, TaxDataError> {
    let raw = field(chars, pos, len);
    if raw.is_empty() {
        return Ok(Money::ZERO);
    }
    let (neg, digits) = match raw.strip_prefix('N') {
        Some(d) => (true, d),
        None => (false, raw.as_str()),
    };
    let cents: i64 = digits
        .parse()
        .map_err(|_| TaxDataError::Format(format!("importe no numérico «{raw}»")))?;
    Ok(Money::from_cents(if neg { -cents } else { cents }))
}

/// ¿El código del registro es exactamente `prefix` seguido del número de orden? (Hay tipos que
/// empiezan igual con otro diseño, p. ej. `CTP` frente a `CT`.)
fn is(code: &str, prefix: &str) -> bool {
    code.strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Lee el fichero de datos fiscales. Un fichero que sólo trae un error (p. ej. «Error 5001
/// Contribuyente no ha ratificado su domicilio fiscal») se devuelve como error.
pub fn parse(bytes: &[u8]) -> Result<TaxData, TaxDataError> {
    let text = taxpain_core::bank::decode(bytes);
    let trimmed = text.trim();
    if trimmed.contains("5001") && trimmed.to_lowercase().contains("ratificado") {
        return Err(TaxDataError::NotRatified);
    }
    if trimmed.starts_with("Error") || trimmed.starts_with("ERROR") {
        return Err(TaxDataError::Service(trimmed.chars().take(200).collect()));
    }
    let mut data = TaxData::default();
    let mut header = false;
    let mut closed = false;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<char> = line.chars().collect();
        match c.first() {
            Some('0') => {
                let code = field(&c, 2, 8);
                let year = code
                    .strip_prefix("DDFF")
                    .and_then(|y| y.parse().ok())
                    .ok_or_else(|| TaxDataError::Format(format!("cabecera «{code}»")))?;
                data.year = year;
                header = true;
            }
            Some('1') => data.process_date = Some(field(&c, 52, 8)).filter(|d| !d.is_empty()),
            Some('9') => closed = true,
            Some('2') => {
                let code = field(&c, 2, 7);
                if is(&code, "RTC") {
                    data.activities.push(ActivityIncome {
                        payer_nif: field(&c, 9, 9),
                        payer_name: field(&c, 18, 40),
                        key: field(&c, 58, 2),
                        income: amount(&c, 60, 13)?,
                        withholding: amount(&c, 73, 13)?,
                    });
                } else if is(&code, "PF") {
                    data.installments.push(Installment {
                        model: field(&c, 9, 3),
                        kind: field(&c, 12, 1),
                        period: field(&c, 13, 2),
                        receipt: field(&c, 17, 13),
                        amount: amount(&c, 30, 13)?,
                    });
                } else if is(&code, "CT") {
                    data.contributions.push(Contribution {
                        regime: field(&c, 21, 4),
                        amount: amount(&c, 25, 17)?,
                    });
                } else if is(&code, "RET") {
                    data.reta_regularization
                        .push((amount(&c, 9, 17)?, amount(&c, 26, 17)?));
                } else {
                    data.other_records += 1;
                }
            }
            _ => data.other_records += 1,
        }
    }
    if !header {
        return Err(TaxDataError::Format("falta el registro de cabecera".into()));
    }
    if !closed {
        return Err(TaxDataError::Format(
            "falta el registro de cierre (¿descarga incompleta?)".into(),
        ));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(s: &str, len: usize) -> String {
        format!("{s:<len$}")
    }

    /// Fichero sintético con el diseño 2025 (NIF de prueba).
    pub fn sample() -> String {
        let rtc = format!(
            "2RTC0001B12345674{}G1{:013}{:013}",
            pad("CLIENTE UNO SL", 40),
            1_000_000,
            150_000
        );
        let rtc2 = format!(
            "2RTC0002A22222228{}G1{:013}N{:012}",
            pad("CLIENTE DOS SA", 40),
            50_000,
            7_500
        );
        let pf = format!("2PF00001130I1T46{}{:013}", pad("1300000000001", 13), 31_080);
        let ct = format!("2CT00001{}0521{:017}", pad("281234567890", 12), 354_000);
        let ret = format!("2RET0001{:017}{:017}", 12_000, 0);
        format!(
            "0DDFF2025\n1{}20260402\n{rtc}\n{rtc2}\n{pf}\n{ct}\n{ret}\n2DN00001X\n2CTP0001000012345S000000000N\n99\n",
            " ".repeat(50)
        )
    }

    #[test]
    fn lee_actividad_pagos_y_cotizaciones() {
        let d = parse(sample().as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.year, 2025);
        assert_eq!(d.process_date.as_deref(), Some("20260402"));
        assert_eq!(d.activities.len(), 2);
        assert_eq!(d.activities[0].payer_nif, "B12345674");
        assert_eq!(d.activities[0].payer_name, "CLIENTE UNO SL");
        assert_eq!(d.activities[0].key, "G1");
        assert_eq!(d.activities[0].income, Money::from_euros(10_000, 0));
        assert_eq!(d.activities[0].withholding, Money::from_euros(1_500, 0));
        assert_eq!(d.activities[1].withholding, -Money::from_euros(75, 0));
        assert_eq!(d.installments[0].model, "130");
        assert_eq!(d.installments[0].period, "1T");
        assert_eq!(d.installments[0].amount, Money::from_cents(31_080));
        assert_eq!(d.contributions[0].regime, "0521");
        assert_eq!(d.contributions[0].amount, Money::from_euros(3_540, 0));
        assert_eq!(
            d.reta_regularization,
            vec![(Money::from_euros(120, 0), Money::ZERO)]
        );
        // DN y CTP (otro diseño que empieza por «CT») no se interpretan y no rompen la lectura.
        assert_eq!(d.other_records, 2);
        assert_eq!(d.contributions.len(), 1);
        // Con CRLF también.
        let crlf = sample().replace('\n', "\r\n");
        assert_eq!(parse(crlf.as_bytes()).ok(), Some(d));
    }

    #[test]
    fn errores() {
        assert_eq!(
            parse(b"Error 5001 Contribuyente no ha ratificado su domicilio fiscal."),
            Err(TaxDataError::NotRatified)
        );
        assert!(matches!(
            parse(b"0DDFF2025\n"),
            Err(TaxDataError::Format(_))
        ));
        assert!(matches!(parse(b"<html>"), Err(TaxDataError::Format(_))));
    }
}
