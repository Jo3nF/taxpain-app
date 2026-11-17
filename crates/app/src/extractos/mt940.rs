//! MT940 (SWIFT, «Customer Statement Message») y MT942, que dan muchos bancos europeos y la
//! banca de empresas.
//!
//! Campos: `:25:` cuenta, `:28C:` número de extracto, `:60F:`/`:60M:` saldo inicial,
//! `:61:` movimiento, `:86:` información del movimiento, `:62F:`/`:62M:` saldo final. Los
//! importes llevan coma decimal y el signo va en la marca (`C` abono, `D` cargo; `RC`/`RD`,
//! anulaciones). Un extracto largo llega en varias páginas (`:60M:`…`:62M:`) que se juntan.

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::{Date, Month};

use super::numeros::importe;
use super::{Extracto, Movimiento, iban};

/// ¿Parece un MT940 o MT942?
pub(crate) fn es_mt940(texto: &str) -> bool {
    let t: String = texto.chars().take(20_000).collect();
    t.contains(":20:") && t.contains(":25:") && (t.contains(":61:") || t.contains(":60F:"))
}

/// Campos del mensaje con sus líneas de continuación unidas.
fn campos(texto: &str) -> Vec<(String, String)> {
    static TAG: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"^:(\d{2}[A-Z]?):(.*)$").ok());
    let Some(re) = TAG.as_ref() else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for linea in texto.lines() {
        let l = linea.trim_end_matches('\r');
        if let Some(c) = re.captures(l)
            && let (Some(t), Some(v)) = (c.get(1), c.get(2))
        {
            out.push((t.as_str().to_owned(), v.as_str().to_owned()));
        } else if let Some((_, v)) = out.last_mut()
            && !l.starts_with("-}")
            && !l.starts_with('{')
            && l.trim() != "-"
        {
            v.push('\n');
            v.push_str(l);
        }
    }
    out
}

fn yymmdd(s: &str) -> Option<Date> {
    let y: i32 = s.get(..2)?.parse().ok()?;
    let m: u8 = s.get(2..4)?.parse().ok()?;
    let d: u8 = s.get(4..6)?.parse().ok()?;
    let y = if y >= 70 { 1900 + y } else { 2000 + y };
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// Saldo `:60F:`/`:62F:`: `C250131EUR1234,56` (marca, fecha, divisa, importe).
fn saldo(v: &str) -> Option<(Money, Date, String)> {
    let v = v.trim();
    let neg = v.starts_with('D');
    let fecha = yymmdd(v.get(1..7)?)?;
    let divisa = v.get(7..10)?.to_owned();
    let imp = importe(v.get(10..)?)?;
    Some((if neg { -imp } else { imp }, fecha, divisa))
}

/// Línea `:61:`: fecha valor `AAMMDD`, fecha de anotación `MMDD` opcional, marca
/// (`C`, `D`, `RC`, `RD`), código de fondos opcional, importe, tipo (`NTRF`…), referencias.
fn linea_61(v: &str) -> Option<(Date, Date, Money, String)> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"^(\d{6})(\d{4})?(RC|RD|C|D)([A-Z])?(\d+,\d{0,2})([A-Z][A-Z0-9]{3})?(.*)").ok()
    });
    let re = RE.as_ref()?;
    let primera = v.lines().next()?;
    let c = re.captures(primera.trim())?;
    let valor = yymmdd(c.get(1)?.as_str())?;
    let anotacion = match c.get(2) {
        Some(md) => {
            let m: u8 = md.as_str().get(..2)?.parse().ok()?;
            let d: u8 = md.as_str().get(2..)?.parse().ok()?;
            // La anotación puede caer en el año de al lado de la fecha valor (31/12 y 02/01).
            let mut f = Date::from_calendar_date(valor.year(), Month::try_from(m).ok()?, d).ok()?;
            if (f - valor).whole_days() > 180 {
                f = f.replace_year(valor.year() - 1).ok()?;
            } else if (valor - f).whole_days() > 180 {
                f = f.replace_year(valor.year() + 1).ok()?;
            }
            f
        }
        None => valor,
    };
    let imp = importe(c.get(5)?.as_str())?;
    let imp = match c.get(3)?.as_str() {
        "D" | "RC" => -imp,
        _ => imp,
    };
    // Referencia del cliente y, tras «//», la del banco; en la segunda línea, más detalle.
    let mut refs = c.get(7).map_or("", |r| r.as_str()).trim().to_owned();
    for extra in v.lines().skip(1) {
        refs.push(' ');
        refs.push_str(extra.trim());
    }
    Some((anotacion, valor, imp, refs.trim().to_owned()))
}

/// Texto del `:86:`, sin los códigos estructurados (`?20`, `/REMI/`…) como separadores.
fn info_86(v: &str) -> String {
    static SUB: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\?\d{2}").ok());
    let t = v.replace('\n', "");
    let t = match SUB.as_ref() {
        Some(re) => re.replace_all(&t, " ").into_owned(),
        None => t,
    };
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lee un MT940: un extracto por cuenta y divisa, con todas sus páginas juntas.
pub(crate) fn leer(texto: &str) -> Vec<Extracto> {
    let mut out: Vec<Extracto> = Vec::new();
    let mut actual: Option<Extracto> = None;
    for (tag, v) in campos(texto) {
        match tag.as_str() {
            "25" => {
                let cuenta = v.trim().to_owned();
                // Una página más de la misma cuenta: sigue en el mismo extracto.
                if actual.as_ref().is_some_and(|e| {
                    e.iban.as_deref() == Some(&iban::compactar(&cuenta))
                        || e.cuenta == clave_25(&cuenta)
                }) {
                    continue;
                }
                if let Some(e) = actual.take() {
                    out.push(e);
                }
                let mut e = Extracto::nuevo("MT940");
                if let Some(i) = iban::en_texto(&cuenta).first() {
                    e.con_iban(i);
                } else {
                    e.cuenta = clave_25(&cuenta);
                }
                actual = Some(e);
            }
            "60F" | "60M" => {
                if let (Some(e), Some((s, f, d))) = (actual.as_mut(), saldo(&v))
                    && e.saldo_inicial.is_none()
                {
                    e.saldo_inicial = Some(s);
                    e.desde = Some(f);
                    e.divisa = d;
                }
            }
            "61" => {
                if let Some(e) = actual.as_mut() {
                    match linea_61(&v) {
                        Some((fecha, valor, imp, referencia)) => e.movimientos.push(Movimiento {
                            importe_eur: None,
                            fecha,
                            fecha_valor: Some(valor),
                            concepto: String::new(),
                            importe: imp,
                            saldo: None,
                            referencia,
                        }),
                        None => e
                            .avisos
                            .push(format!("línea :61: que no se entiende: {}", v.trim())),
                    }
                }
            }
            "86" => {
                if let Some(m) = actual.as_mut().and_then(|e| e.movimientos.last_mut())
                    && m.concepto.is_empty()
                {
                    m.concepto = info_86(&v);
                }
            }
            "62F" | "62M" => {
                if let (Some(e), Some((s, f, _))) = (actual.as_mut(), saldo(&v)) {
                    e.saldo_final = Some(s);
                    e.hasta = Some(f);
                }
            }
            _ => {}
        }
    }
    if let Some(e) = actual {
        out.push(e);
    }
    for e in &mut out {
        for m in &mut e.movimientos {
            if m.concepto.is_empty() {
                m.concepto = m.referencia.clone();
            }
        }
    }
    out
}

/// Clave de la cuenta del `:25:` cuando no es un IBAN («21000418/0200051332», «BIC/cuenta»).
fn clave_25(cuenta: &str) -> String {
    let digitos: String = cuenta.chars().filter(char::is_ascii_digit).collect();
    if digitos.len() == 20 {
        format!("{}{}{}", &digitos[..4], &digitos[4..8], &digitos[10..])
    } else if digitos.len() == 18 {
        digitos
    } else {
        cuenta.trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const MT: &str = "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:STMT0001\n\
:25:ES9121000418450200051332\n:28C:1/1\n:60F:C241231EUR1000,00\n\
:61:2501020102D45,30NTRFNONREF//B1\n:86:RECIBO LUZ ENERO\n\
:61:2501100110C1200,NTRFFACT-7\n:86:?20TRANSFERENCIA ?21CLIENTE DEMO\n:62M:C250110EUR2154,70\n-}\n\
{1:F01DEMOESMMAXXX0000000000}{4:\n:20:STMT0002\n:25:ES9121000418450200051332\n:28C:1/2\n\
:60M:C250110EUR2154,70\n:61:250131D4,70NCHGNONREF\n:86:COMISION\n:62F:C250131EUR2150,00\n-}";

    #[test]
    fn lee_un_mt940_de_dos_paginas() {
        assert!(es_mt940(MT));
        let e = leer(MT);
        assert_eq!(e.len(), 1);
        let e = &e[0];
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.saldo_inicial.map(|m| m.cents()), Some(100_000));
        assert_eq!(e.saldo_final.map(|m| m.cents()), Some(215_000));
        assert_eq!(e.desde, Some(date!(2024 - 12 - 31)));
        assert_eq!(e.hasta, Some(date!(2025 - 01 - 31)));
        assert_eq!(e.movimientos.len(), 3);
        assert_eq!(e.movimientos[0].importe.cents(), -4530);
        assert_eq!(e.movimientos[0].concepto, "RECIBO LUZ ENERO");
        assert_eq!(e.movimientos[1].importe.cents(), 120_000);
        assert_eq!(e.movimientos[1].concepto, "TRANSFERENCIA CLIENTE DEMO");
        assert_eq!(e.movimientos[2].fecha, date!(2025 - 01 - 31));
        let mut e = e.clone();
        assert!(e.cerrar().cuadra());
    }

    #[test]
    fn anotacion_en_el_año_siguiente() {
        let (f, v, imp, _) = linea_61("2412310102RD10,00NTRFX").unwrap();
        assert_eq!((f, v), (date!(2025 - 01 - 02), date!(2024 - 12 - 31)));
        assert_eq!(imp.cents(), 1000);
    }
}
