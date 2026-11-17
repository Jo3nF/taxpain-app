//! QIF (Quicken Interchange Format): texto con un campo por línea y `^` al final de cada
//! movimiento. `D` fecha, `T` (o `U`) importe con signo, `P` beneficiario, `M` nota, `N` número
//! o referencia. La cabecera `!Type:Bank`, `!Type:CCard`… dice qué cuenta es; `!Account` con su
//! `N` da el nombre. No trae número de cuenta ni saldos (salvo, a veces, un primer movimiento
//! «Opening Balance»).

use super::numeros::{Orden, fecha, importe, inferir_orden};
use super::{Clase, Extracto, Movimiento};

/// ¿Parece un QIF?
pub(crate) fn es_qif(texto: &str) -> bool {
    texto
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("!type:")
        || texto
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("!account")
}

/// Fecha QIF: `12/31/2025`, `12/31'25`, `31/12/2025`, `2025-12-31`, `1/ 5'25`…
fn fecha_qif(s: &str, orden: Orden) -> Option<time::Date> {
    let s = s.replace('\'', "/").replace(' ', "");
    fecha(&s, None, orden)
}

/// Lee un QIF. `nombre` (el del fichero) da la clave de la cuenta si el QIF no trae una.
pub(crate) fn leer(texto: &str, nombre: &str) -> Vec<Extracto> {
    let texto = texto.trim_start_matches('\u{feff}');
    // El orden de día y mes se deduce de todas las fechas del fichero.
    let fechas: Vec<String> = texto
        .lines()
        .filter_map(|l| l.strip_prefix('D'))
        .map(|d| d.replace('\'', "/").replace(' ', ""))
        .collect();
    let orden = inferir_orden(fechas.iter().map(String::as_str));
    let mut out: Vec<Extracto> = Vec::new();
    let mut cuenta_nombre = String::new();
    let mut en_cuenta = false;
    let mut actual: Option<Extracto> = None;
    let mut campos: Vec<(char, String)> = Vec::new();
    let cerrar_actual = |actual: &mut Option<Extracto>, out: &mut Vec<Extracto>| {
        if let Some(e) = actual.take()
            && !e.movimientos.is_empty()
        {
            out.push(e);
        }
    };
    for linea in texto.lines() {
        let l = linea.trim_end_matches('\r');
        if l.trim().is_empty() {
            continue;
        }
        let lower = l.to_ascii_lowercase();
        if lower.starts_with("!account") {
            en_cuenta = true;
            continue;
        }
        if let Some(tipo) = lower.strip_prefix("!type:") {
            en_cuenta = false;
            cerrar_actual(&mut actual, &mut out);
            let mut e = Extracto::nuevo("QIF");
            e.clase = match tipo.trim() {
                "ccard" => Clase::TarjetaCredito,
                _ => Clase::Cuenta,
            };
            let base = if cuenta_nombre.is_empty() {
                nombre
            } else {
                &cuenta_nombre
            };
            e.cuenta = format!("QIF:{}", base.trim());
            e.avisos.push(
                "El QIF no trae el número de cuenta: comprueba que va a la cuenta correcta."
                    .to_owned(),
            );
            actual = Some(e);
            continue;
        }
        if l.starts_with('!') {
            continue;
        }
        if en_cuenta {
            if let Some(n) = l.strip_prefix('N') {
                cuenta_nombre = n.trim().to_owned();
            }
            if l.starts_with('^') {
                en_cuenta = false;
            }
            continue;
        }
        if l.starts_with('^') {
            let e = actual.get_or_insert_with(|| {
                let mut e = Extracto::nuevo("QIF");
                e.cuenta = format!("QIF:{}", nombre.trim());
                e
            });
            let campo = |c: char| {
                campos
                    .iter()
                    .find(|(k, _)| *k == c)
                    .map(|(_, v)| v.trim().to_owned())
            };
            let f = campo('D').and_then(|d| fecha_qif(&d, orden));
            let imp = campo('T').or_else(|| campo('U')).and_then(|t| importe(&t));
            let concepto = [campo('P'), campo('M')]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            match (f, imp) {
                (Some(f), Some(imp))
                    if e.movimientos.is_empty()
                        && concepto.to_ascii_lowercase().starts_with("opening balance") =>
                {
                    e.saldo_inicial = Some(imp);
                    e.desde = Some(f);
                }
                (Some(f), Some(imp)) => e.movimientos.push(Movimiento {
                    importe_eur: None,
                    fecha: f,
                    fecha_valor: None,
                    concepto,
                    importe: imp,
                    saldo: None,
                    referencia: campo('N').unwrap_or_default(),
                }),
                _ => e
                    .avisos
                    .push("un movimiento del QIF sin fecha o sin importe".to_owned()),
            }
            campos.clear();
            continue;
        }
        let mut chars = l.chars();
        if let Some(c) = chars.next() {
            campos.push((c, chars.as_str().to_owned()));
        }
    }
    cerrar_actual(&mut actual, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn lee_un_qif() {
        let t = "!Account\nNCuenta Demo\nTBank\n^\n!Type:Bank\nD01/02'25\nT1,000.00\n\
                 POpening Balance\n^\nD01/15'25\nT-45.30\nPRECIBO LUZ\nMenero\nN123\n^\n\
                 D01/20'25\nT1,200.00\nPCLIENTE DEMO\n^\n";
        assert!(es_qif(t));
        let e = leer(t, "demo.qif");
        assert_eq!(e.len(), 1);
        let e = &e[0];
        assert_eq!(e.cuenta, "QIF:Cuenta Demo");
        assert_eq!(e.saldo_inicial.map(|m| m.cents()), Some(100_000));
        assert_eq!(e.movimientos.len(), 2);
        assert_eq!(e.movimientos[0].fecha, date!(2025 - 01 - 15));
        assert_eq!(e.movimientos[0].importe.cents(), -4530);
        assert_eq!(e.movimientos[0].concepto, "RECIBO LUZ · enero");
        assert_eq!(e.movimientos[0].referencia, "123");
        assert_eq!(e.movimientos[1].importe.cents(), 120_000);
    }

    #[test]
    fn qif_europeo_de_tarjeta() {
        let t = "!Type:CCard\nD31/01/2025\nT-12,50\nPLIBRERIA\n^\n";
        let e = leer(t, "tarjeta.qif");
        assert_eq!(e[0].clase, Clase::TarjetaCredito);
        assert_eq!(e[0].cuenta, "QIF:tarjeta.qif");
        assert_eq!(e[0].movimientos[0].fecha, date!(2025 - 01 - 31));
        assert_eq!(e[0].movimientos[0].importe.cents(), -1250);
    }
}
