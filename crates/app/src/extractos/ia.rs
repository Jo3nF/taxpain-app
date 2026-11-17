//! Extractos leídos con IA: el asistente con la clave del usuario («Leer con IA») o Claude Code
//! con `/leer-carpeta`. Es el último recurso, para los formatos que no reconoce ningún lector,
//! y sus datos se comprueban con los saldos igual que los demás antes de importarlos.
//!
//! La lectura (`tipo` = `extracto_bancario`) trae una de estas dos cosas:
//! - `hoja` (hojas de cálculo): las columnas de `fecha`, `fecha_valor`, `concepto` (y, si hay
//!   más, `concepto_2`, `beneficiario`, `observaciones`), `importe` (con signo) o `cargo` y
//!   `abono`, `saldo` y `divisa`;
//! - `extracto` (PDF, fotos…): `{"cuenta": {"iban", "numero", "banco", "titular", "divisa",
//!   "clase", "desde", "hasta", "saldo_inicial", "saldo_final"}, "movimientos": [[fecha,
//!   fecha_valor, concepto, importe, saldo], …]}` (cada movimiento también puede ser un objeto
//!   con esas claves). Con varias cuentas, `extractos`: una lista de bloques así.

use serde_json::Value;
use taxpain_core::money::Money;

use super::numeros::{Orden, fecha, importe, inferir_orden};
use super::{Clase, Extracto, Movimiento, iban};
use crate::import::Hoja;
use crate::lectura_ia::{Reading, SheetPlan};

/// Texto de un valor JSON (texto o número), sin vacíos ni `null`.
fn texto(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
    .filter(|s| !s.is_empty() && s != "null")
}

fn dinero(v: Option<&Value>) -> Option<Money> {
    importe(&texto(v)?)
}

fn dia(v: Option<&Value>) -> Option<time::Date> {
    fecha(&texto(v)?, None, Orden::DiaMes)
}

fn clase(s: &str) -> Clase {
    let s = s.to_lowercase();
    if s.contains("credito") || s.contains("crédito") {
        Clase::TarjetaCredito
    } else if s.contains("debito") || s.contains("débito") || s.contains("tarjeta") {
        Clase::TarjetaDebito
    } else if s.contains("monedero") || s.contains("paypal") {
        Clase::Monedero
    } else {
        Clase::Cuenta
    }
}

/// Los datos de la cuenta de un bloque `cuenta`.
fn cuenta(e: &mut Extracto, c: &serde_json::Map<String, Value>) {
    if let Some(b) = texto(c.get("banco")) {
        e.banco = b;
    }
    let numero = texto(c.get("iban")).or_else(|| texto(c.get("numero")));
    if let Some(n) = numero {
        if iban::valido(&n) {
            e.con_iban(&n);
        } else if let Some(i) = iban::desde_ccc(&n) {
            e.con_iban(&i);
        } else if let Some(m) = iban::enmascarada(&n) {
            e.cuenta = m.clave();
        } else {
            e.cuenta = iban::compactar(&n);
        }
    }
    if let Some(t) = texto(c.get("titular")) {
        e.titular = t;
    }
    if let Some(d) = texto(c.get("divisa")) {
        e.divisa = d.to_uppercase();
    }
    if let Some(k) = texto(c.get("clase")) {
        e.clase = clase(&k);
    }
    e.desde = dia(c.get("desde"));
    e.hasta = dia(c.get("hasta"));
    e.saldo_inicial = dinero(c.get("saldo_inicial"));
    e.saldo_final = dinero(c.get("saldo_final"));
}

/// Un bloque `extracto` (cuenta y movimientos).
fn bloque(b: &Value, nombre: &str) -> Option<Extracto> {
    let mut e = Extracto::nuevo("Lectura con IA");
    if let Some(c) = b.get("cuenta").and_then(Value::as_object) {
        cuenta(&mut e, c);
    }
    if e.cuenta.is_empty() {
        e.cuenta = format!("ia:{nombre}");
        e.avisos
            .push("La lectura no trae el número de cuenta: comprueba la cuenta.".to_owned());
    }
    for m in b.get("movimientos").and_then(Value::as_array)? {
        let campo = |i: usize, k: &str| match m {
            Value::Array(a) => a.get(i),
            Value::Object(o) => o.get(k),
            _ => None,
        };
        let (Some(f), Some(imp)) = (dia(campo(0, "fecha")), dinero(campo(3, "importe"))) else {
            e.avisos
                .push("un movimiento de la lectura sin fecha o sin importe".to_owned());
            continue;
        };
        e.movimientos.push(Movimiento {
            importe_eur: None,
            fecha: f,
            fecha_valor: dia(campo(1, "fecha_valor")),
            concepto: texto(campo(2, "concepto")).unwrap_or_default(),
            importe: imp,
            saldo: dinero(campo(4, "saldo")),
            referencia: String::new(),
        });
    }
    Some(e)
}

/// Columna de una letra de la hoja («A», «AB») o de un número («3»).
fn columna(letra: &str) -> Option<usize> {
    let l = letra.trim().to_ascii_uppercase();
    if l.bytes().all(|b| b.is_ascii_digit()) {
        return l.parse::<usize>().ok()?.checked_sub(1);
    }
    if l.is_empty() || !l.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let mut n = 0usize;
    for b in l.bytes() {
        n = n * 26 + usize::from(b - b'A' + 1);
    }
    n.checked_sub(1)
}

/// Las hojas de un extracto con las columnas que señaló el asistente.
fn con_plan(hojas: &[Hoja], plan: &SheetPlan, nombre: &str) -> Option<Extracto> {
    let col = |k: &str| plan.columnas.get(k).and_then(|l| columna(l));
    let fecha_c = col("fecha")?;
    let (imp_c, cargo_c, abono_c) = (col("importe"), col("cargo"), col("abono"));
    if imp_c.is_none() && cargo_c.is_none() && abono_c.is_none() {
        return None;
    }
    let conceptos: Vec<usize> = ["concepto", "concepto_2", "beneficiario", "observaciones"]
        .iter()
        .filter_map(|k| col(k))
        .collect();
    let mut e = Extracto::nuevo("Hoja de cálculo (columnas según la IA)");
    let celda = |f: &[String], c: Option<usize>| -> String {
        c.and_then(|c| f.get(c))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    // Los datos de la cuenta, de lo que hay encima de la tabla.
    let encima: String = hojas
        .iter()
        .flat_map(|h| {
            h.filas
                .iter()
                .take(plan.fila_titulos.max(plan.primera_fila))
        })
        .map(|f| f.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(i) = iban::en_texto(&encima).first() {
        e.con_iban(i);
    } else {
        e.cuenta = format!("ia:{nombre}");
    }
    let primera = plan.primera_fila.max(1) - 1;
    for h in hojas {
        let filas: Vec<&Vec<String>> = h.filas.iter().skip(primera).collect();
        let orden = inferir_orden(
            filas
                .iter()
                .filter_map(|f| f.get(fecha_c))
                .map(String::as_str),
        );
        for f in filas {
            let Some(dia_) = fecha(&celda(f, Some(fecha_c)), None, orden) else {
                continue;
            };
            let imp = match imp_c {
                Some(c) => importe(&celda(f, Some(c))),
                None => {
                    let cargo = importe(&celda(f, cargo_c)).map(|m| -m.abs());
                    let abono = importe(&celda(f, abono_c)).map(Money::abs);
                    match (cargo, abono) {
                        (Some(c), Some(a)) if c.is_zero() => Some(a),
                        (Some(c), _) if !c.is_zero() => Some(c),
                        (_, a) => a,
                    }
                }
            };
            let Some(imp) = imp else {
                continue;
            };
            let concepto = conceptos
                .iter()
                .map(|c| celda(f, Some(*c)))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            e.movimientos.push(Movimiento {
                importe_eur: None,
                fecha: dia_,
                fecha_valor: col("fecha_valor")
                    .and_then(|c| fecha(&celda(f, Some(c)), None, orden)),
                concepto,
                importe: imp,
                saldo: col("saldo").and_then(|c| importe(&celda(f, Some(c)))),
                referencia: String::new(),
            });
            if let Some(d) = col("divisa")
                .map(|c| celda(f, Some(c)))
                .filter(|d| d.len() == 3)
            {
                e.divisa = d.to_uppercase();
            }
        }
    }
    // Del más nuevo al más antiguo: se da la vuelta entera, para no desordenar el día.
    if e.movimientos
        .first()
        .zip(e.movimientos.last())
        .is_some_and(|(a, b)| a.fecha > b.fecha)
    {
        e.movimientos.reverse();
    }
    Some(e)
}

/// Extractos de una lectura de IA de tipo `extracto_bancario`, si trae datos con los que
/// leerlo (`None` si sólo dice qué es).
pub(crate) fn desde_lectura(r: &Reading, nombre: &str, bytes: &[u8]) -> Option<Vec<Extracto>> {
    if r.tipo != "extracto_bancario" {
        return None;
    }
    if let Some(x) = &r.extracto {
        let bloques: Vec<&Value> = match x.get("extractos").and_then(Value::as_array) {
            Some(l) => l.iter().collect(),
            None => vec![x],
        };
        let out: Vec<Extracto> = bloques
            .into_iter()
            .filter_map(|b| bloque(b, nombre))
            .collect();
        return (!out.is_empty()).then_some(out);
    }
    let plan = r.hoja.as_ref()?;
    let hojas = crate::import::sheets(nombre, bytes).ok()?;
    con_plan(&hojas, plan, nombre).map(|e| vec![e])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractos::Cuadre;
    use time::macros::date;

    fn lectura(json: &str) -> Reading {
        crate::lectura_ia::reading_from_value(&serde_json::from_str(json).unwrap()).unwrap()
    }

    #[test]
    fn extracto_con_movimientos_en_listas() {
        let r = lectura(
            r#"{"tipo": "extracto_bancario", "descripcion": "Extracto",
            "extracto": {"cuenta": {"iban": "ES91 2100 0418 4502 0005 1332", "titular":
            "EMPRESA DEMO SL", "divisa": "EUR", "desde": "2025-01-01", "hasta": "2025-01-31",
            "saldo_inicial": "1000.00", "saldo_final": "2154.70"},
            "movimientos": [["2025-01-03", "2025-01-03", "RECIBO LUZ", "-45.30", "954.70"],
                            {"fecha": "2025-01-10", "concepto": "CLIENTE DEMO",
                             "importe": 1200, "saldo": "2154.70"}]}}"#,
        );
        let mut e = desde_lectura(&r, "x.pdf", b"").unwrap();
        assert_eq!(e.len(), 1);
        let e = &mut e[0];
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.movimientos[1].importe.cents(), 120_000);
        assert_eq!(e.movimientos[0].fecha_valor, Some(date!(2025 - 01 - 03)));
        assert_eq!(e.cerrar(), Cuadre::Cuadra);
    }

    #[test]
    fn hoja_con_cargo_y_abono() {
        let csv = "EMPRESA DEMO SL;ES9121000418450200051332\n\nFecha;Detalle;Cargo;Abono;Saldo\n\
                   10/01/2025;CLIENTE DEMO;;1.200,00;2.154,70\n03/01/2025;RECIBO LUZ;45,30;;954,70\n";
        let r = lectura(
            r#"{"tipo": "extracto_bancario", "hoja": {"fila_titulos": 3, "primera_fila": 4,
            "columnas": {"fecha": "A", "concepto": "B", "cargo": "C", "abono": "D", "saldo": "E"}}}"#,
        );
        let mut e = desde_lectura(&r, "movimientos.csv", csv.as_bytes()).unwrap();
        let e = &mut e[0];
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.movimientos.len(), 2);
        assert_eq!(e.movimientos[0].importe.cents(), -4530);
        assert_eq!(e.cerrar(), Cuadre::Cuadra);
        assert_eq!(e.saldo_inicial.map(|m| m.cents()), Some(100_000));
    }

    #[test]
    fn sin_datos_no_hay_extracto() {
        let r = lectura(r#"{"tipo": "extracto_bancario", "descripcion": "Extracto"}"#);
        assert_eq!(desde_lectura(&r, "x.pdf", b""), None);
    }
}
