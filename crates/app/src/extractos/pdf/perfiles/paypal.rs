//! PayPal: el «Extracto bancario» de una cuenta de empresa, en PDF.
//!
//! La primera página trae el periodo, el titular y el «Resumen de saldo» con una fila por
//! divisa (saldo inicial y final disponibles, y los retenidos). La segunda, el «Resumen de
//! actividad» (pagos recibidos, tarifas…). Al final, un «Historial de transacciones - XXX» por
//! cada divisa con movimientos, en el que el lector de PDF parte cada transacción en tres
//! líneas: `d/m/aaaa <descripción>`, `Id.: <referencia> <nombre>` y
//! `<correo> <bruto> <comisión> <neto>`. Las cabeceras de página salen al final de cada página.
//!
//! Cada cobro se guarda como dos movimientos con la misma referencia: el importe bruto y, si la
//! hay, la comisión de PayPal en negativo. Así el bruto casa con la factura cobrada, la comisión
//! se puede contabilizar como gasto y la suma de los dos es el neto que entra en el saldo.

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::Date;

use super::comun::{al_derecho, cantidad, fecha, importe, limpiar, sin_punto};
use crate::extractos::{Clase, Extracto, Movimiento};

static EXTRACTO_DE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Extracto bancario de (.+?) al (.+)$").ok());
static RESUMEN_SALDO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^Resumen de saldo \((\d{1,2}/\d{1,2}/\d{4}) - (\d{1,2}/\d{1,2}/\d{4})\)").ok()
});
static SALDOS: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^([A-Z]{3}) (-?\d{1,3}(?:\.\d{3})*,\d{2}) (-?\d{1,3}(?:\.\d{3})*,\d{2}) -?\d{1,3}(?:\.\d{3})*,\d{2} -?\d{1,3}(?:\.\d{3})*,\d{2}$",
    )
    .ok()
});
static VENDEDOR: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"Id\. de cuenta de vendedor: (\S+)").ok());
static HISTORIAL: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Historial de transacciones - ([A-Z]{3})$").ok());
static TRANSACCION: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2}/\d{1,2}/\d{4}) (.+)$").ok());
static ID: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^Id\.: (\S+) ?(.*)$").ok());
static RECIBIDOS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Pagos recibidos (-?\d{1,3}(?:\.\d{3})*,\d{2})$").ok());
static TARIFAS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Tarifas (-?\d{1,3}(?:\.\d{3})*,\d{2})$").ok());

/// ¿Es un importe suelto («-1.234,56»)?
fn es_importe(p: &str) -> bool {
    let p = p.strip_prefix('-').unwrap_or(p);
    p.split_once(',').is_some_and(|(e, d)| {
        d.len() == 2
            && d.bytes().all(|b| b.is_ascii_digit())
            && !e.is_empty()
            && e.split('.')
                .all(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Una transacción del historial, con sus líneas.
struct Transaccion {
    fecha: Date,
    descripcion: String,
    lineas: Vec<String>,
}

/// Lo que se va leyendo de una divisa.
struct Divisa {
    codigo: String,
    inicial: Option<Money>,
    final_: Option<Money>,
    transacciones: Vec<Transaccion>,
    historial: bool,
}

/// Extractos de PayPal, uno por divisa, si el texto es de uno.
pub(super) fn leer(texto: &str) -> Option<Vec<Extracto>> {
    if !texto.contains("Id. de cuenta de vendedor:")
        || !texto.contains("Historial de transacciones - ")
        || !texto.contains("PayPal")
    {
        return None;
    }
    let mut desde = None;
    let mut hasta = None;
    let mut titular = String::new();
    let mut vendedor = String::new();
    let mut recibidos = None;
    let mut tarifas = None;
    let mut divisas: Vec<Divisa> = Vec::new();
    let mut actual: Option<usize> = None;
    let mut abierta: Option<Transaccion> = None;
    let mut esperando_titular = false;
    for l in texto.lines() {
        let t = l.trim();
        if t.is_empty() {
            cerrar(&mut abierta, actual, &mut divisas);
            continue;
        }
        if esperando_titular {
            titular = sin_punto(t);
            esperando_titular = false;
            continue;
        }
        if let Some(c) = EXTRACTO_DE.as_ref()?.captures(t) {
            desde = fecha(&c[1]);
            hasta = fecha(&c[2]);
            esperando_titular = true;
            continue;
        }
        if let Some(c) = RESUMEN_SALDO.as_ref()?.captures(t) {
            desde = desde.or(fecha(&c[1]));
            hasta = hasta.or(fecha(&c[2]));
            continue;
        }
        if let Some(c) = VENDEDOR.as_ref()?.captures(t) {
            cerrar(&mut abierta, actual, &mut divisas);
            vendedor = c[1].to_owned();
            continue;
        }
        if t.starts_with("PayPal (Europe)") || t.starts_with("Página") {
            cerrar(&mut abierta, actual, &mut divisas);
            continue;
        }
        if let Some(c) = HISTORIAL.as_ref()?.captures(t) {
            cerrar(&mut abierta, actual, &mut divisas);
            let i = divisa(&mut divisas, &c[1]);
            divisas[i].historial = true;
            actual = Some(i);
            continue;
        }
        if actual.is_none() {
            // Antes del historial: los resúmenes.
            if let Some(c) = SALDOS.as_ref()?.captures(t) {
                let i = divisa(&mut divisas, &c[1]);
                divisas[i].inicial = importe(&c[2]);
                divisas[i].final_ = importe(&c[3]);
            } else if let Some(c) = RECIBIDOS.as_ref()?.captures(t) {
                recibidos = recibidos.or(importe(&c[1]));
            } else if let Some(c) = TARIFAS.as_ref()?.captures(t) {
                tarifas = tarifas.or(importe(&c[1]));
            }
            continue;
        }
        if t.starts_with("Fecha Descripción") {
            continue;
        }
        if let Some(c) = TRANSACCION.as_ref()?.captures(t)
            && let Some(f) = fecha(&c[1])
        {
            cerrar(&mut abierta, actual, &mut divisas);
            abierta = Some(Transaccion {
                fecha: f,
                descripcion: limpiar(&c[2]),
                lineas: Vec::new(),
            });
            continue;
        }
        if let Some(a) = abierta.as_mut() {
            a.lineas.push(t.to_owned());
        }
    }
    cerrar(&mut abierta, actual, &mut divisas);
    let una_divisa = divisas.iter().filter(|d| d.historial).count() == 1;
    let out: Vec<Extracto> = divisas
        .into_iter()
        .filter(|d| d.historial || d.final_.is_some_and(|f| !f.is_zero()))
        .map(|d| {
            let mut e = Extracto::nuevo("PDF");
            e.banco = "PayPal".to_owned();
            e.clase = Clase::Monedero;
            e.cuenta = format!("paypal:{vendedor}");
            e.titular = titular.clone();
            e.divisa = d.codigo.clone();
            e.desde = desde;
            e.hasta = hasta;
            e.saldo_inicial = d.inicial;
            e.saldo_final = d.final_;
            for tr in &d.transacciones {
                movimientos(&mut e, tr);
            }
            al_derecho(&mut e.movimientos, false);
            if una_divisa && d.historial {
                comprobar_resumen(&mut e, recibidos, tarifas);
            }
            e
        })
        .collect();
    Some(out)
}

/// Índice de la divisa, que se añade si no estaba.
fn divisa(divisas: &mut Vec<Divisa>, codigo: &str) -> usize {
    if let Some(i) = divisas.iter().position(|d| d.codigo == codigo) {
        return i;
    }
    divisas.push(Divisa {
        codigo: codigo.to_owned(),
        inicial: None,
        final_: None,
        transacciones: Vec::new(),
        historial: false,
    });
    divisas.len() - 1
}

fn cerrar(abierta: &mut Option<Transaccion>, actual: Option<usize>, divisas: &mut [Divisa]) {
    if let (Some(a), Some(i)) = (abierta.take(), actual)
        && let Some(d) = divisas.get_mut(i)
    {
        d.transacciones.push(a);
    }
}

/// Los movimientos de una transacción: el bruto y la comisión.
fn movimientos(e: &mut Extracto, tr: &Transaccion) {
    let mut referencia = String::new();
    let mut nombre = String::new();
    let mut correo = String::new();
    let mut importes: Vec<Money> = Vec::new();
    for l in &tr.lineas {
        if let Some(c) = ID.as_ref().and_then(|re| re.captures(l)) {
            referencia = c[1].to_owned();
            nombre = limpiar(&c[2]);
            continue;
        }
        for p in l.split_whitespace() {
            if es_importe(p) {
                if let Some(m) = importe(p) {
                    importes.push(m);
                }
            } else if p.contains('@') {
                correo = p.to_owned();
            }
        }
    }
    let [bruto, comision, neto] = match importes.as_slice() {
        [.., b, c, n] => [*b, *c, *n],
        _ => {
            e.avisos.push(format!(
                "No se han encontrado los importes de la transacción del {} («{}»): no se ha \
                 leído.",
                tr.fecha, tr.descripcion
            ));
            return;
        }
    };
    if bruto + comision != neto {
        e.avisos.push(format!(
            "La transacción del {} ({}) no cuadra: bruto {} + comisión {} no da el neto {}.",
            tr.fecha,
            referencia,
            cantidad(bruto, &e.divisa),
            cantidad(comision, &e.divisa),
            cantidad(neto, &e.divisa)
        ));
    }
    let concepto = [tr.descripcion.as_str(), nombre.as_str(), correo.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ");
    e.movimientos.push(Movimiento {
        importe_eur: None,
        fecha: tr.fecha,
        fecha_valor: None,
        concepto,
        importe: bruto,
        saldo: None,
        referencia: referencia.clone(),
    });
    if !comision.is_zero() {
        e.movimientos.push(Movimiento {
            importe_eur: None,
            fecha: tr.fecha,
            fecha_valor: None,
            concepto: "Comisión PayPal".to_owned(),
            importe: comision,
            saldo: None,
            referencia,
        });
    }
}

/// Compara los cobros y las comisiones con el «Resumen de actividad».
fn comprobar_resumen(e: &mut Extracto, recibidos: Option<Money>, tarifas: Option<Money>) {
    let cobros: Money = e
        .movimientos
        .iter()
        .filter(|m| m.concepto != "Comisión PayPal" && !m.importe.is_negative())
        .map(|m| m.importe)
        .sum();
    let comisiones: Money = e
        .movimientos
        .iter()
        .filter(|m| m.concepto == "Comisión PayPal")
        .map(|m| m.importe)
        .sum();
    if let Some(r) = recibidos
        && r != cobros
    {
        e.avisos.push(format!(
            "Los pagos recibidos del resumen ({}) no coinciden con los del historial ({}).",
            cantidad(r, &e.divisa),
            cantidad(cobros, &e.divisa)
        ));
    }
    if let Some(t) = tarifas
        && t != comisiones
    {
        e.avisos.push(format!(
            "Las tarifas del resumen ({}) no coinciden con las comisiones del historial ({}).",
            cantidad(t, &e.divisa),
            cantidad(comisiones, &e.divisa)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const EXTRACTO: &[&str] = &[
        "",
        "",
        "Extracto bancario de 01 enero 2026 al 31 marzo 2026",
        " ",
        "EMPRESA DEMO SL ",
        "Calle Ficticia, 1 ",
        "Edificio Demo ",
        "46000 Valencia  ",
        "Resumen de saldo (1/1/2026 - 31/3/2026) ",
        " Saldo inicial disponible Saldo final disponible Saldo inicial retenido Saldo final \
         retenido",
        "",
        "EUR 10,00 130,53 0,00 0,00",
        "",
        "USD 0,00 0,00 0,00 0,00",
        "",
        "Id. de cuenta de vendedor: DEMO0000000XY Id. de PayPal: pagos@empresademo.example \
         1/1/2026 - 31/3/2026",
        "",
        "Página  1",
        "",
        "Resumen de actividad (1/1/2026 - 31/3/2026) ",
        " EUR",
        "",
        "Saldo inicial disponible 10,00",
        "",
        "Pagos recibidos 125,50",
        "",
        "Pagos enviados 0,00",
        "",
        "Retiradas y cargos 0,00",
        "",
        "Depósitos y créditos 0,00",
        "",
        "Tarifas -4,97",
        "",
        "Saldo final disponible 130,53",
        "",
        "Id. de cuenta de vendedor: DEMO0000000XY Id. de PayPal: pagos@empresademo.example \
         1/1/2026 - 31/3/2026",
        "",
        "Página  2",
        "",
        "Pagos recibidos ",
        "",
        " ",
        "Tarifas ",
        " Descripción EUR",
        "",
        "Pago estándar 125,50",
        "",
        "Total 125,50",
        "",
        "Descripción EUR",
        "",
        "Tarifa de pago -4,97",
        "",
        "Total -4,97",
        "",
        "Id. de cuenta de vendedor: DEMO0000000XY Id. de PayPal: pagos@empresademo.example \
         1/1/2026 - 31/3/2026",
        "",
        "Página  3",
        "",
        "Historial de transacciones - EUR",
        "",
        "Fecha Descripción Nombre \\ Correo electrónico Bruto Comisión Neto",
        "",
        "12/1/2026 Pago estándar",
        "Id.: 1AB23456CD7890123 Cliente Demo Uno SL",
        "cliente1@example.com 100,00 -3,75 96,25",
        "",
        "3/2/2026 Pago estándar",
        "Id.: 2BC34567DE8901234 Cliente Demo Dos",
        "cliente2@example.com 25,50 -1,22 24,28",
        "",
        "PayPal (Europe) S.à r.l. et Cie , S.C.A., 22-24 Boulevard Royal, L-2449 Luxembourg. \
         R.C.S. Luxemburgo",
        "",
        "Id. de cuenta de vendedor: DEMO0000000XY Id. de PayPal: pagos@empresademo.example \
         1/1/2026 - 31/3/2026",
        "",
        "Página  4",
    ];

    #[test]
    fn extracto_de_paypal() {
        let v = leer(&EXTRACTO.join("\n")).unwrap();
        // La fila de dólares, a cero y sin historial, no da otro extracto.
        assert_eq!(v.len(), 1);
        let mut e = v[0].clone();
        assert_eq!(e.formato, "PDF");
        assert_eq!(e.banco, "PayPal");
        assert_eq!(e.clase, Clase::Monedero);
        assert_eq!(e.cuenta, "paypal:DEMO0000000XY");
        assert_eq!(e.iban, None);
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.desde, Some(date!(2026 - 01 - 01)));
        assert_eq!(e.hasta, Some(date!(2026 - 03 - 31)));
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(1_000)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(13_053)));
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        let filas: Vec<_> = e
            .movimientos
            .iter()
            .map(|m| {
                (
                    m.fecha,
                    m.importe.cents(),
                    m.concepto.as_str(),
                    m.referencia.as_str(),
                    m.saldo,
                    m.fecha_valor,
                )
            })
            .collect();
        assert_eq!(
            filas,
            [
                (
                    date!(2026 - 01 - 12),
                    10_000,
                    "Pago estándar · Cliente Demo Uno SL · cliente1@example.com",
                    "1AB23456CD7890123",
                    None,
                    None
                ),
                (
                    date!(2026 - 01 - 12),
                    -375,
                    "Comisión PayPal",
                    "1AB23456CD7890123",
                    None,
                    None
                ),
                (
                    date!(2026 - 02 - 03),
                    2_550,
                    "Pago estándar · Cliente Demo Dos · cliente2@example.com",
                    "2BC34567DE8901234",
                    None,
                    None
                ),
                (
                    date!(2026 - 02 - 03),
                    -122,
                    "Comisión PayPal",
                    "2BC34567DE8901234",
                    None,
                    None
                ),
            ]
        );
        assert!(e.cerrar().cuadra());
    }

    #[test]
    fn avisa_si_el_resumen_no_coincide() {
        let t = EXTRACTO
            .join("\n")
            .replace("Tarifas -4,97", "Tarifas -5,00");
        let e = &leer(&t).unwrap()[0];
        assert_eq!(e.avisos.len(), 1, "{:?}", e.avisos);
    }

    #[test]
    fn sin_la_huella_de_paypal_no_es_suyo() {
        let t = EXTRACTO
            .join("\n")
            .replace("Historial de transacciones", "Historial");
        assert!(leer(&t).is_none());
    }
}
