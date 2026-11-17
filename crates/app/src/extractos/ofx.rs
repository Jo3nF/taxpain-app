//! OFX y QFX (Open Financial Exchange): la versión 1 (SGML, con las etiquetas de los valores
//! sin cerrar) y la 2 (XML). Lo exportan muchos bancos y programas de finanzas.
//!
//! Se leen los extractos de cuenta (`STMTRS`) y de tarjeta (`CCSTMTRS`): la cuenta
//! (`BANKACCTFROM` / `CCACCTFROM`), la divisa (`CURDEF`), los movimientos (`STMTTRN`: `DTPOSTED`,
//! `DTUSER`, `TRNAMT` con signo, `FITID`, `NAME`, `MEMO`) y el saldo final (`LEDGERBAL`).

use super::numeros::{Orden, fecha, importe, luhn};
use super::{Clase, Extracto, Movimiento, iban};

/// ¿Parece un OFX?
pub(crate) fn es_ofx(texto: &str) -> bool {
    let cabeza: String = texto.chars().take(4000).collect::<String>().to_uppercase();
    cabeza.contains("OFXHEADER") || cabeza.contains("<OFX>") || cabeza.contains("<?OFX")
}

/// Texto entre `<TAG>` y `</TAG>` (sin distinguir mayúsculas), desde `desde`. Devuelve el
/// contenido y la posición siguiente al cierre.
fn bloque<'a>(t: &'a str, up: &str, tag: &str, desde: usize) -> Option<(&'a str, usize)> {
    let abre = format!("<{tag}>");
    let cierra = format!("</{tag}>");
    let ini = up.get(desde..)?.find(&abre)? + desde + abre.len();
    let resto = up.get(ini..)?;
    // Sin cierre (SGML descuidado), el bloque acaba donde empieza el siguiente.
    let (fin, salto) = match (resto.find(&cierra), resto.find(&abre)) {
        (Some(c), Some(a)) if a < c => (ini + a, 0),
        (Some(c), _) => (ini + c, cierra.len()),
        (None, Some(a)) => (ini + a, 0),
        (None, None) => (up.len(), 0),
    };
    Some((t.get(ini..fin)?, (fin + salto).min(up.len())))
}

/// Todos los bloques `<TAG>…</TAG>` de un texto.
fn bloques<'a>(t: &'a str, tag: &str) -> Vec<&'a str> {
    // En mayúsculas ASCII para no mover las posiciones.
    let up = t.to_ascii_uppercase();
    let mut out = Vec::new();
    let mut desde = 0;
    while let Some((b, sig)) = bloque(t, &up, tag, desde) {
        out.push(b);
        if sig <= desde {
            break;
        }
        desde = sig;
    }
    out
}

/// Valor de una etiqueta hoja (`<TAG>valor`, cerrada o no), sin entidades XML.
fn valor(t: &str, tag: &str) -> Option<String> {
    let up = t.to_ascii_uppercase();
    let abre = format!("<{tag}>");
    let ini = up.find(&abre)? + abre.len();
    let resto = t.get(ini..)?;
    let v = resto.split('<').next()?.trim();
    (!v.is_empty()).then(|| {
        v.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    })
}

/// Fecha OFX: `AAAAMMDD[HHMMSS[.XXX]][[-5:EST]]`.
fn fecha_ofx(s: &str) -> Option<time::Date> {
    fecha(s.get(..8)?, None, Orden::DiaMes)
}

/// Lee todos los extractos de un OFX (uno por cuenta).
pub(crate) fn leer(texto: &str) -> Vec<Extracto> {
    let mut out = Vec::new();
    for (tag, tarjeta) in [("STMTRS", false), ("CCSTMTRS", true)] {
        for b in bloques(texto, tag) {
            let mut e = Extracto::nuevo("OFX");
            if tarjeta {
                e.clase = Clase::TarjetaCredito;
            }
            if let Some(d) = valor(b, "CURDEF") {
                e.divisa = d.to_uppercase();
            }
            let cuenta = bloques(
                b,
                if tarjeta {
                    "CCACCTFROM"
                } else {
                    "BANKACCTFROM"
                },
            );
            let cuenta = cuenta.first().copied().unwrap_or(b);
            let acct = valor(cuenta, "ACCTID").unwrap_or_default();
            let bank = valor(cuenta, "BANKID").unwrap_or_default();
            let branch = valor(cuenta, "BRANCHID").unwrap_or_default();
            if iban::valido(&acct) {
                e.con_iban(&acct);
            } else {
                let digitos = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
                let (a, b4, o4) = (digitos(&acct), digitos(&bank), digitos(&branch));
                let entidad = iban::nombre_entidad(&b4).map(|_| b4.as_str());
                if let Some(n) = entidad.and_then(iban::nombre_entidad) {
                    e.banco = n.to_owned();
                }
                // Una tarjeta (o un número de tarjeta como cuenta): la clave de los demás
                // lectores, «tarjeta:<entidad>:<últimos>», nunca el número entero (los primeros
                // dígitos son del emisor, iguales para muchas, y el PAN no se guarda).
                let pan = (13..=19).contains(&a.len()) && luhn(&a);
                if (tarjeta || pan) && a.len() >= 4 {
                    e.clase = Clase::TarjetaCredito;
                    e.con_tarjeta(entidad, &a[a.len() - 4..]);
                } else if a.len() == 20 {
                    // Código cuenta cliente español: entidad, oficina, control y cuenta.
                    e.cuenta = format!("{}{}{}", &a[..4], &a[4..8], &a[10..]);
                } else if b4.len() == 4 && o4.len() == 4 && a.len() == 10 {
                    e.cuenta = format!("{b4}{o4}{a}");
                } else {
                    e.cuenta = format!("{bank}{branch}{acct}");
                }
            }
            if let Some(lista) = bloques(b, "BANKTRANLIST").first() {
                e.desde = valor(lista, "DTSTART").as_deref().and_then(fecha_ofx);
                e.hasta = valor(lista, "DTEND").as_deref().and_then(fecha_ofx);
                for t in bloques(lista, "STMTTRN") {
                    let (Some(f), Some(imp)) = (
                        valor(t, "DTPOSTED").as_deref().and_then(fecha_ofx),
                        valor(t, "TRNAMT").as_deref().and_then(importe),
                    ) else {
                        e.avisos
                            .push("un movimiento del OFX sin fecha o sin importe".to_owned());
                        continue;
                    };
                    let usuario = valor(t, "DTUSER").as_deref().and_then(fecha_ofx);
                    let concepto = [valor(t, "NAME"), valor(t, "MEMO")]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();
                    let concepto = if concepto.len() == 2 && concepto[1].contains(&concepto[0]) {
                        concepto[1].clone()
                    } else {
                        concepto.join(" · ")
                    };
                    e.movimientos.push(Movimiento {
                        importe_eur: None,
                        // La fecha de la operación es la del usuario si la hay; la de cargo en
                        // cuenta, la fecha valor.
                        fecha: usuario.unwrap_or(f),
                        fecha_valor: Some(f),
                        concepto,
                        importe: imp,
                        saldo: None,
                        referencia: valor(t, "FITID")
                            .or_else(|| valor(t, "REFNUM"))
                            .or_else(|| valor(t, "CHECKNUM"))
                            .unwrap_or_default(),
                    });
                }
            }
            if let Some(l) = bloques(b, "LEDGERBAL").first() {
                e.saldo_final = valor(l, "BALAMT").as_deref().and_then(importe);
            }
            out.push(e);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const SGML: &str = "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n<OFX>\n<BANKMSGSRSV1>\
<STMTTRNRS><STMTRS><CURDEF>EUR\n<BANKACCTFROM><BANKID>2100<BRANCHID>0418\
<ACCTID>ES9121000418450200051332<ACCTTYPE>CHECKING</BANKACCTFROM>\n<BANKTRANLIST>\
<DTSTART>20250101<DTEND>20250131\n<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20250103120000[+1:CET]\
<TRNAMT>-45.30<FITID>A1<NAME>RECIBO LUZ<MEMO>Factura enero</STMTTRN>\n<STMTTRN>\
<TRNTYPE>CREDIT<DTPOSTED>20250110<TRNAMT>1200.00<FITID>A2<NAME>CLIENTE DEMO &amp; CIA\
</STMTTRN>\n</BANKTRANLIST><LEDGERBAL><BALAMT>2154.70<DTASOF>20250131</LEDGERBAL>\
</STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>";

    #[test]
    fn lee_un_ofx_sgml() {
        assert!(es_ofx(SGML));
        let e = leer(SGML);
        assert_eq!(e.len(), 1);
        let e = &e[0];
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.banco, "CaixaBank");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.desde, Some(date!(2025 - 01 - 01)));
        assert_eq!(e.movimientos.len(), 2);
        assert_eq!(e.movimientos[0].importe.cents(), -4530);
        assert_eq!(e.movimientos[0].fecha, date!(2025 - 01 - 03));
        assert_eq!(e.movimientos[0].concepto, "RECIBO LUZ · Factura enero");
        assert_eq!(e.movimientos[1].concepto, "CLIENTE DEMO & CIA");
        assert_eq!(e.saldo_final.map(|m| m.cents()), Some(215_470));
    }

    #[test]
    fn lee_un_ofx_xml_de_tarjeta() {
        let x = r#"<?xml version="1.0"?><?OFX OFXHEADER="200" VERSION="220"?><OFX>
<CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><CURDEF>USD</CURDEF>
<CCACCTFROM><ACCTID>4111111111111111</ACCTID></CCACCTFROM><BANKTRANLIST>
<STMTTRN><DTPOSTED>20250205</DTPOSTED><DTUSER>20250203</DTUSER><TRNAMT>-19.99</TRNAMT>
<FITID>X9</FITID><NAME>SOFTWARE DEMO</NAME></STMTTRN></BANKTRANLIST>
</CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>"#;
        let e = leer(x);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].clase, Clase::TarjetaCredito);
        // La clave de las tarjetas de todos los lectores, nunca el número entero.
        assert_eq!(e[0].cuenta, "tarjeta:?:1111");
        assert_eq!(e[0].divisa, "USD");
        assert_eq!(e[0].movimientos[0].fecha, date!(2025 - 02 - 03));
        assert_eq!(e[0].movimientos[0].fecha_valor, Some(date!(2025 - 02 - 05)));
        assert_eq!(e[0].movimientos[0].referencia, "X9");
        // Con la entidad en BANKID, con ella (y el banco); un número de tarjeta en un extracto
        // de cuenta también es una tarjeta.
        let con_banco = x
            .replace("<CCACCTFROM>", "<CCACCTFROM><BANKID>0182</BANKID>")
            .replace("CCSTMTRS", "STMTRS")
            .replace("CCACCTFROM", "BANKACCTFROM");
        let e = leer(&con_banco);
        assert_eq!(e[0].cuenta, "tarjeta:0182:1111");
        assert_eq!(e[0].banco, "BBVA");
        assert_eq!(e[0].clase, Clase::TarjetaCredito);
    }
}
