//! La empresa año a año con lo que tiene la AEAT («Comprobar lo presentado»): las cifras clave
//! de las declaraciones vigentes (no las sustituidas por una complementaria) de cada ejercicio y,
//! si TaxPain lleva la contabilidad de ese año, las suyas al lado. Cada cifra dice de qué modelo
//! y casilla sale y cuánto cambia frente al año anterior; donde no hay dato, no se inventa.
//!
//! Casillas (comprobadas con declaraciones reales, leídas con el diseño de registro de su año):
//! - 390: [108] volumen de operaciones, [47] total cuotas devengadas, [64] suma de deducciones,
//!   [86] resultado de la liquidación. Sin 390, la suma de los 303: [27], [45] y [71]; el
//!   volumen, del 303 del 4T de los exonerados del 390 ([88]).
//! - 349: total del registro de declarante de cada periodo.
//! - 111: [28] total de retenciones; si no, el 190. 115: [03]; si no, el 180.
//! - 123: hasta 2023 (diseño de 2015) [02] base y [03] retenciones; desde 2024 [06] y [09]; si no,
//!   el 193.
//! - 202: [03] pago fraccionado.
//! - 200: [00255] cifra de negocios, [00270] gastos de personal, [00279] otros gastos de
//!   explotación, [00296] resultado de explotación, [00501] resultado antes de impuestos (el de
//!   la liquidación: la [00325] de la cuenta de resultados se repite en la página 12 con otro
//!   sentido), [00500] resultado contable, [00547] bases negativas compensadas, [00552] base
//!   imponible, [00562] cuota íntegra, [01586] resultado de la autoliquidación, [00180] total
//!   activo, [00186] fondos propios y [00177] efectivo. Cada 200 se lee con el diseño de registro
//!   de su ejercicio (`lectura_justificante::from_file_200`).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use time::Date;

use crate::error::Result;
use crate::lectura_justificante::FilingBoxes;
use crate::presentado_aeat::{self, Vigente};

/// Una cifra de un año.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Valor {
    pub año: i32,
    /// En céntimos (o el número, en las filas de recuentos). `None`: sin dato.
    pub valor: Option<i64>,
    /// De dónde sale («390 [108]», «suma de los 303 [27]»…).
    pub fuente: Option<String>,
    /// Diferencia con el año anterior (si los dos tienen dato).
    pub diferencia: Option<i64>,
    /// La misma, en décimas de tanto por ciento del año anterior (si no era cero).
    pub variacion: Option<i64>,
    /// Avance de un año sin cerrar (sin su declaración anual): no se compara con el anterior.
    pub avance: bool,
}

/// Comienzo de la fuente de un avance.
const AVANCE: &str = "avance";

/// Bases de las ventas de un 303: devengado interior al 21, 10 y 4 % ([01], [04], [07]), al 0, 5
/// y 2 % de los años de rebaja del aceite y los alimentos ([150], [153], [165]), rectificaciones
/// ([14]) y, de la información adicional, entregas intracomunitarias y exportaciones ([59],
/// [60]), no sujetas por localización ([120]) e inversión del sujeto pasivo ([122]).
/// `TODO(VERIFY)`: casillas [150], [153] y [165] con los diseños de registro de 2023 a 2025.
fn ventas_303(b: &FilingBoxes) -> Money {
    [1, 4, 7, 150, 153, 165, 14, 59, 60, 120, 122]
        .iter()
        .map(|c| b.get(*c))
        .sum()
}

/// Una cifra a lo largo de los años.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fila {
    pub id: &'static str,
    pub titulo: &'static str,
    /// Qué es, para alguien que no es asesor.
    pub ayuda: &'static str,
    /// `true` si es un recuento (no euros).
    pub recuento: bool,
    pub valores: Vec<Valor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Grupo {
    pub titulo: &'static str,
    pub filas: Vec<Fila>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Evolucion {
    /// Ejercicios, del más antiguo al más reciente.
    pub años: Vec<i32>,
    pub grupos: Vec<Grupo>,
    /// No hay ninguna declaración de la AEAT guardada (hay que comprobar lo presentado).
    pub sin_comprobar: bool,
    /// Declaraciones vigentes sin casillas leídas (sus cifras salen como «sin dato»).
    pub sin_casillas: usize,
}

/// Las declaraciones vigentes de cada año, por modelo.
struct Datos<'a> {
    por_año: BTreeMap<(i32, String), Vec<&'a Vigente>>,
}

type Dato = Option<(Money, String)>;

/// Resultado [71] de un 303 como el del periodo entero: el de una complementaria es sólo la
/// diferencia con la anterior ([71] = [69] − [70] + [109]), así que se le vuelven a sumar lo
/// ingresado antes ([70]) y se le restan las devoluciones anteriores ([109]).
fn resultado_303(b: &FilingBoxes) -> Money {
    let mut m: std::collections::BTreeMap<u16, Money> = b
        .casillas
        .iter()
        .filter_map(|(k, v)| Some((u16::try_from(*k).ok()?, *v)))
        .collect();
    crate::presentados::whole_period("303", &mut m);
    m.get(&71).copied().unwrap_or(Money::ZERO)
}

/// Resultado del periodo sin las compensaciones de otros periodos: la suma de resultados [64]
/// (devengado menos deducible); si no está (casillas a cero no se guardan), la del régimen
/// general [46].
fn resultado_303_sin_compensar(b: &FilingBoxes) -> Money {
    b.casillas
        .get(&64)
        .or_else(|| b.casillas.get(&46))
        .copied()
        .unwrap_or(Money::ZERO)
}

impl<'a> Datos<'a> {
    fn de(&self, model: &str, y: i32) -> &[&'a Vigente] {
        self.por_año
            .get(&(y, model.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Las casillas de las declaraciones de ese modelo y año que se han leído.
    fn leidas(&self, model: &str, y: i32) -> Vec<&'a FilingBoxes> {
        self.de(model, y)
            .iter()
            .filter_map(|v| v.boxes.as_ref())
            .collect()
    }

    /// Una casilla de la declaración anual (o del periodo `period`).
    fn casilla(&self, model: &str, y: i32, period: &str, c: u32) -> Dato {
        let v = self
            .de(model, y)
            .iter()
            .find(|v| v.period.eq_ignore_ascii_case(period))?;
        let b = v.boxes.as_ref()?;
        Some((b.get(c), format!("{model} [{}]", fmt_casilla(model, c))))
    }

    /// Los periodos de las declaraciones de ese modelo y año que se han leído.
    fn periodos_leidos(&self, model: &str, y: i32) -> Vec<&'a str> {
        self.de(model, y)
            .iter()
            .copied()
            .filter(|v| v.boxes.is_some())
            .map(|v| v.period.as_str())
            .collect()
    }

    /// La suma de una casilla en los periodos del año (si se leyó alguno).
    fn suma(&self, model: &str, y: i32, c: impl Fn(&FilingBoxes) -> Money, que: &str) -> Dato {
        let l = self.leidas(model, y);
        if l.is_empty() {
            return None;
        }
        let n = self.de(model, y).len();
        let parcial = if l.len() < n {
            format!(" ({} de {n} periodos leídos)", l.len())
        } else {
            String::new()
        };
        Some((l.iter().map(|b| c(b)).sum(), format!("{que}{parcial}")))
    }

    /// Un total del registro de declarante de una informativa anual.
    fn informativa(&self, model: &str, y: i32, f: impl Fn(&FilingBoxes) -> Option<Money>) -> Dato {
        let b = self.leidas(model, y).into_iter().next()?;
        Some((f(b)?, format!("{model} (total)")))
    }
}

/// [00255] en el 200; en el resto, el número tal cual.
fn fmt_casilla(model: &str, c: u32) -> String {
    if model == "200" {
        format!("{c:05}")
    } else {
        format!("{c:02}")
    }
}

fn o(a: Dato, b: impl FnOnce() -> Dato) -> Dato {
    a.or_else(b)
}

/// Una fila: su cifra de cada año con la diferencia frente al anterior.
fn fila(
    id: &'static str,
    titulo: &'static str,
    ayuda: &'static str,
    años: &[i32],
    f: impl Fn(i32) -> Option<(i64, String)>,
    recuento: bool,
) -> Option<Fila> {
    let mut valores = Vec::new();
    let mut antes: Option<(i32, i64)> = None;
    for &y in años {
        let d = f(y);
        let valor = d.as_ref().map(|(v, _)| *v);
        let avance = d.as_ref().is_some_and(|(_, f)| f.starts_with(AVANCE));
        let (diferencia, variacion) = match (antes, valor) {
            _ if avance => (None, None),
            (Some((ya, a)), Some(v)) if ya == y - 1 => (
                Some(v - a),
                (a != 0).then(|| ((v - a) as i128 * 1000 / i128::from(a.abs())) as i64),
            ),
            _ => (None, None),
        };
        antes = valor.map(|v| (y, v));
        valores.push(Valor {
            año: y,
            valor,
            fuente: d.map(|(_, f)| f),
            diferencia,
            variacion,
            avance,
        });
    }
    valores.iter().any(|v| v.valor.is_some()).then_some(Fila {
        id,
        titulo,
        ayuda,
        recuento,
        valores,
    })
}

fn euros(d: Dato) -> Option<(i64, String)> {
    d.map(|(m, f)| (m.cents(), f))
}

/// La evolución de la empresa con lo guardado (no consulta la AEAT). `hoy` para lo que está
/// fuera de plazo.
pub fn evolucion(v: &Vault, hoy: Date) -> Result<Evolucion> {
    let vigentes = presentado_aeat::vigentes(v)?;
    let mut por_año: BTreeMap<(i32, String), Vec<&Vigente>> = BTreeMap::new();
    for x in &vigentes {
        por_año
            .entry((x.year, x.model.clone()))
            .or_default()
            .push(x);
    }
    let d = Datos { por_año };
    let contables = años_contables(v)?;
    let mut años: BTreeSet<i32> = vigentes.iter().map(|x| x.year).collect();
    años.extend(contables.iter().copied());
    let años: Vec<i32> = años.into_iter().collect();

    let iva = [
        fila(
            "volumen",
            "Volumen de operaciones",
            "Lo facturado en el año a efectos del IVA (ventas y servicios), según el resumen anual.",
            &años,
            |y| {
                euros(o(d.casilla("390", y, "0A", 108), || {
                    d.casilla("303", y, "4T", 88)
                }))
            },
            false,
        ),
        fila(
            "iva_devengado",
            "IVA devengado",
            "El IVA de las ventas (y el de las compras con inversión del sujeto pasivo).",
            &años,
            |y| {
                euros(o(d.casilla("390", y, "0A", 47), || {
                    d.suma("303", y, |b| b.get(27), "suma de los 303 [27]")
                }))
            },
            false,
        ),
        fila(
            "iva_deducible",
            "IVA deducible",
            "El IVA de las compras y gastos que se ha restado.",
            &años,
            |y| {
                euros(o(d.casilla("390", y, "0A", 64), || {
                    d.suma("303", y, |b| b.get(45), "suma de los 303 [45]")
                }))
            },
            false,
        ),
        fila(
            "iva_resultado",
            "Resultado del IVA del año",
            "IVA devengado menos deducible del año, sin lo compensado de otros periodos. \
             Positivo: sale a ingresar; negativo: a compensar o devolver.",
            &años,
            |y| {
                euros(o(d.casilla("390", y, "0A", 84), || {
                    d.suma(
                        "303",
                        y,
                        resultado_303_sin_compensar,
                        "suma de los 303 [64]",
                    )
                }))
            },
            false,
        ),
        fila(
            "intracomunitarias",
            "Operaciones intracomunitarias",
            "Compras y ventas con empresas de otros países de la UE declaradas en el 349.",
            &años,
            |y| {
                euros(d.suma(
                    "349",
                    y,
                    |b| b.importe.unwrap_or(Money::ZERO),
                    "suma de los 349",
                ))
            },
            false,
        ),
    ];
    let retenciones = [
        fila(
            "ret_trabajo",
            "Retenciones de nóminas y profesionales",
            "Lo retenido a trabajadores, administradores y profesionales e ingresado en Hacienda.",
            &años,
            |y| {
                euros(o(
                    d.suma("111", y, |b| b.get(28), "suma de los 111 [28]"),
                    || d.informativa("190", y, |b| b.retenciones),
                ))
            },
            false,
        ),
        fila(
            "perceptores",
            "Perceptores con retención",
            "Personas a las que se retuvo en el año, según el resumen anual 190.",
            &años,
            |y| {
                d.leidas("190", y)
                    .first()
                    .and_then(|b| b.registros)
                    .map(|n| (i64::from(n), "190 (registros)".to_owned()))
            },
            true,
        ),
        fila(
            "ret_alquiler",
            "Retenciones de alquileres",
            "Lo retenido al arrendador de un local e ingresado en Hacienda.",
            &años,
            |y| {
                euros(o(
                    d.suma("115", y, |b| b.get(3), "suma de los 115 [03]"),
                    || d.informativa("180", y, |b| b.retenciones),
                ))
            },
            false,
        ),
        fila(
            "dividendos",
            "Dividendos y otras rentas del capital pagadas",
            "Lo repartido a socios o pagado como intereses, antes de la retención.",
            &años,
            |y| {
                let c = if y < 2024 { 2 } else { 6 };
                let que = format!("suma de los 123 [{c:02}]");
                euros(o(d.suma("123", y, |b| b.get(c), &que), || {
                    d.informativa("193", y, |b| b.importe)
                }))
            },
            false,
        ),
        fila(
            "ret_capital",
            "Retenciones del capital",
            "Lo retenido sobre esos dividendos o intereses e ingresado en Hacienda.",
            &años,
            |y| {
                let c = if y < 2024 { 3 } else { 9 };
                let que = format!("suma de los 123 [{c:02}]");
                euros(o(d.suma("123", y, |b| b.get(c), &que), || {
                    d.informativa("193", y, |b| b.retenciones)
                }))
            },
            false,
        ),
    ];
    let pagos_fraccionados = fila(
        "pagos_fraccionados",
        "Pagos fraccionados del Impuesto sobre Sociedades",
        "Lo adelantado del impuesto en abril, octubre y diciembre (202).",
        &años,
        |y| {
            euros(d.suma(
                "202",
                y,
                |b| b.get(3) + b.get(34),
                "suma de los 202 ([03] o [34])",
            ))
        },
        false,
    );
    let d = &d;
    let c200 = |id, titulo, ayuda, c: u32| {
        fila(
            id,
            titulo,
            ayuda,
            &años,
            move |y| euros(d.casilla("200", y, "0A", c)),
            false,
        )
    };
    // Lo primero, lo más importante: cuánto vende y gana la empresa y cómo está.
    let actividad = [
        fila(
            "cifra_negocios",
            "Cifra de negocios",
            "Ventas y servicios del año según la cuenta de pérdidas y ganancias del 200. En los \
             años sin 200 (el del año en curso se presenta en julio del siguiente), un avance con \
             las bases de las ventas de los 303 presentados.",
            &años,
            |y| {
                euros(o(d.casilla("200", y, "0A", 255), || {
                    if !d.de("200", y).is_empty() {
                        return None;
                    }
                    let mut ps = d.periodos_leidos("303", y);
                    ps.sort_unstable();
                    d.suma(
                        "303",
                        y,
                        ventas_303,
                        &format!("{AVANCE} con los 303 {} (sin 200)", periodos(&ps)),
                    )
                }))
            },
            false,
        ),
        c200(
            "resultado_explotacion",
            "Resultado de explotación",
            "Lo que gana o pierde la actividad, antes de lo financiero y de impuestos.",
            296,
        ),
        c200("resultado_antes", "Resultado antes de impuestos", "", 501),
        c200(
            "resultado",
            "Resultado contable",
            "El beneficio (o la pérdida) del año según las cuentas.",
            500,
        ),
        c200(
            "fondos_propios",
            "Fondos propios",
            "Capital, reservas y resultado: lo que es de los socios.",
            186,
        ),
        c200(
            "efectivo",
            "Efectivo",
            "Dinero en caja y bancos al cierre.",
            177,
        ),
        c200(
            "total_activo",
            "Total activo",
            "Todo lo que tiene la empresa según el balance.",
            180,
        ),
        c200(
            "gastos_personal",
            "Gastos de personal",
            "Sueldos, Seguridad Social a cargo de la empresa y otros gastos de personal.",
            270,
        ),
        c200(
            "otros_gastos",
            "Otros gastos de explotación",
            "Alquileres, servicios, suministros, tributos…",
            279,
        ),
    ];
    let sociedades = [
        c200(
            "bin",
            "Bases negativas compensadas",
            "Pérdidas de años anteriores restadas este año.",
            547,
        ),
        c200(
            "base_imponible",
            "Base imponible",
            "Sobre lo que se calcula el impuesto.",
            552,
        ),
        c200(
            "cuota_integra",
            "Cuota íntegra",
            "El impuesto antes de deducciones, retenciones y pagos fraccionados.",
            562,
        ),
        pagos_fraccionados,
        c200(
            "resultado_is",
            "Resultado del Impuesto sobre Sociedades",
            "Positivo: a ingresar; negativo: a devolver.",
            1586,
        ),
    ];
    let mut late: BTreeMap<i32, usize> = BTreeMap::new();
    let comprobados: BTreeSet<i32> = presentado_aeat::checked_years(v)?.into_iter().collect();
    for &y in &años {
        if comprobados.contains(&y) {
            let r = presentado_aeat::report(v, y, hoy)?;
            late.insert(
                y,
                r.rows
                    .iter()
                    .filter(|x| x.status == presentado_aeat::Status::Presented && x.late)
                    .count(),
            );
        }
    }
    let declaraciones = [
        fila(
            "presentadas",
            "Declaraciones presentadas",
            "Las que tiene la AEAT de ese ejercicio (una complementaria cuenta una vez).",
            &años,
            |y| {
                let n = vigentes.iter().filter(|x| x.year == y).count();
                (n > 0).then(|| (n as i64, "AEAT".to_owned()))
            },
            true,
        ),
        fila(
            "fuera_de_plazo",
            "Presentadas fuera de plazo",
            "Pueden llevar recargo: mira «Qué revisar».",
            &años,
            |y| {
                late.get(&y)
                    .map(|n| (*n as i64, "comprobación con la AEAT".to_owned()))
            },
            true,
        ),
    ];
    let mut cuentas: BTreeMap<i32, crate::accounting::AnnualAccounts> = BTreeMap::new();
    for &y in &contables {
        cuentas.insert(y, crate::accounting::annual_accounts(v, y)?);
    }
    let tp = |id, titulo, ayuda, f: fn(&crate::accounting::AnnualAccounts) -> Money| {
        fila(
            id,
            titulo,
            ayuda,
            &años,
            |y| {
                cuentas
                    .get(&y)
                    .map(|c| (f(c).cents(), "contabilidad de TaxPain".to_owned()))
            },
            false,
        )
    };
    let taxpain = [
        tp(
            "tp_ventas",
            "Ventas (contabilidad de TaxPain)",
            "Importe neto de la cifra de negocios de la cuenta de resultados que calcula TaxPain.",
            |c| c.income.get("1"),
        ),
        tp(
            "tp_explotacion",
            "Resultado de explotación (TaxPain)",
            "",
            |c| c.income.get("A"),
        ),
        tp(
            "tp_resultado",
            "Resultado del ejercicio (TaxPain)",
            "Antes de regularizar y cerrar el ejercicio.",
            |c| c.income.get("D"),
        ),
        tp("tp_activo", "Total activo (TaxPain)", "", |c| {
            c.balance.get("TA")
        }),
        tp("tp_patrimonio", "Patrimonio neto (TaxPain)", "", |c| {
            c.balance.get("PA")
        }),
    ];
    let grupo = |titulo, filas: Vec<Option<Fila>>| {
        let filas: Vec<Fila> = filas.into_iter().flatten().collect();
        (!filas.is_empty()).then_some(Grupo { titulo, filas })
    };
    // «Dividendos» en el título sólo si la empresa los ha pagado (123 o 193).
    let con_dividendos = retenciones
        .iter()
        .flatten()
        .any(|f| f.id == "dividendos" || f.id == "ret_capital");
    let retenciones_titulo = if con_dividendos {
        "Retenciones y dividendos"
    } else {
        "Retenciones"
    };
    // De lo más importante a lo menos; la contabilidad de TaxPain, al final (suele estar a medias).
    let grupos = [
        grupo("Actividad y cuentas", actividad.into_iter().collect()),
        grupo(
            "Impuesto sobre Sociedades",
            sociedades.into_iter().collect(),
        ),
        grupo("IVA", iva.into_iter().collect()),
        grupo(retenciones_titulo, retenciones.into_iter().collect()),
        grupo("Declaraciones", declaraciones.into_iter().collect()),
        grupo("Contabilidad de TaxPain", taxpain.into_iter().collect()),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok(Evolucion {
        sin_comprobar: vigentes.is_empty(),
        sin_casillas: vigentes.iter().filter(|x| x.boxes.is_none()).count(),
        años,
        grupos,
    })
}

/// Una cifra de lo presentado a la AEAT en un año (para el panel cuando TaxPain no tiene la
/// contabilidad de ese año).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CifraPresentada {
    pub id: &'static str,
    pub titulo: &'static str,
    /// Suma de los periodos leídos, en céntimos.
    pub valor: Money,
    /// De dónde sale («suma de los 303 [27]», con los periodos leídos si faltan).
    pub fuente: String,
    /// Periodos que suma («1T a 3T», «1P y 2P»).
    pub periodos: String,
}

/// Lo presentado a la AEAT en `year` según las declaraciones vigentes con casillas leídas: las
/// mismas sumas que la evolución (IVA de los 303, retenciones del 111 y del 115 y pagos
/// fraccionados del 202), de los periodos presentados hasta hoy; el resultado del IVA, separado
/// en lo que salió a ingresar y lo que salió a compensar o devolver. Sin casillas leídas, vacío.
pub fn presentado_del_año(v: &Vault, year: i32) -> Result<Vec<CifraPresentada>> {
    let vigentes = presentado_aeat::vigentes(v)?;
    let mut por_año: BTreeMap<(i32, String), Vec<&Vigente>> = BTreeMap::new();
    for x in vigentes.iter().filter(|x| x.year == year) {
        por_año
            .entry((x.year, x.model.clone()))
            .or_default()
            .push(x);
    }
    let d = Datos { por_año };
    let cifra = |id, titulo, model: &str, dato: Dato| {
        dato.map(|(valor, fuente)| CifraPresentada {
            id,
            titulo,
            valor,
            fuente,
            periodos: periodos(&d.periodos_leidos(model, year)),
        })
    };
    Ok([
        cifra(
            "iva_devengado",
            "IVA devengado",
            "303",
            d.suma("303", year, get(27), "suma de los 303 [27]"),
        ),
        cifra(
            "iva_deducible",
            "IVA deducible",
            "303",
            d.suma("303", year, get(45), "suma de los 303 [45]"),
        ),
        // El [71] no se suma tal cual: el de un periodo ya resta lo que se compensa de otro
        // anterior ([78]), así que un 1T a compensar más un 2T que lo compensa contaría dos
        // veces lo mismo. Se separan los periodos a ingresar de los que salieron a compensar.
        resultado_iva(&d, year, true),
        resultado_iva(&d, year, false),
        cifra(
            "ret_trabajo",
            "Retenciones de nóminas y profesionales",
            "111",
            d.suma("111", year, get(28), "suma de los 111 [28]"),
        ),
        cifra(
            "ret_alquiler",
            "Retenciones de alquileres",
            "115",
            d.suma("115", year, get(3), "suma de los 115 [03]"),
        ),
        cifra(
            "pagos_fraccionados",
            "Pagos fraccionados del Impuesto sobre Sociedades",
            "202",
            d.suma(
                "202",
                year,
                |b| b.get(3) + b.get(34),
                "suma de los 202 ([03] o [34])",
            ),
        ),
    ]
    .into_iter()
    .flatten()
    .collect())
}

/// La suma del resultado [71] (del periodo entero) de los 303 del año que salieron a ingresar
/// (`a_ingresar`) o a compensar o devolver (en positivo), con sus periodos.
fn resultado_iva(d: &Datos<'_>, year: i32, a_ingresar: bool) -> Option<CifraPresentada> {
    let todos = d.de("303", year);
    let leidos: Vec<(&str, Money)> = todos
        .iter()
        .filter_map(|v| Some((v.period.as_str(), resultado_303(v.boxes.as_ref()?))))
        .collect();
    let de_este: Vec<(&str, Money)> = leidos
        .iter()
        .copied()
        .filter(|(_, r)| {
            if a_ingresar {
                r.cents() > 0
            } else {
                r.cents() < 0
            }
        })
        .collect();
    if de_este.is_empty() {
        return None;
    }
    let parcial = if leidos.len() < todos.len() {
        format!(" ({} de {} periodos leídos)", leidos.len(), todos.len())
    } else {
        String::new()
    };
    let valor: Money = de_este.iter().map(|(_, r)| *r).sum();
    let ps: Vec<&str> = de_este.iter().map(|(p, _)| *p).collect();
    Some(if a_ingresar {
        CifraPresentada {
            id: "iva_a_ingresar",
            titulo: "IVA a ingresar",
            valor,
            fuente: format!("suma de los 303 [71] a ingresar{parcial}"),
            periodos: periodos(&ps),
        }
    } else {
        CifraPresentada {
            id: "iva_a_compensar",
            titulo: "IVA a compensar o devolver",
            valor: -valor,
            fuente: format!("suma de los 303 [71] negativos{parcial}"),
            periodos: periodos(&ps),
        }
    })
}

/// Una casilla de una declaración.
fn get(c: u32) -> impl Fn(&FilingBoxes) -> Money {
    move |b| b.get(c)
}

/// «1T», «1T y 2T», «1T a 3T», «1T, 3T y 4T»: seguidos (tres o más) con «a».
fn periodos(ps: &[&str]) -> String {
    let mut ps: Vec<&str> = ps.to_vec();
    ps.dedup();
    let num = |p: &str| {
        p.trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse::<u32>()
            .ok()
    };
    let suffix = |p: &str| {
        p.trim_start_matches(|c: char| c.is_ascii_digit())
            .to_owned()
    };
    let seguidos = ps.len() >= 3
        && ps.windows(2).all(|w| {
            suffix(w[0]) == suffix(w[1])
                && matches!((num(w[0]), num(w[1])), (Some(a), Some(b)) if b == a + 1)
        });
    match ps.as_slice() {
        [] => String::new(),
        [p] => (*p).to_owned(),
        [first, .., last] if seguidos => format!("{first} a {last}"),
        [init @ .., last] => format!("{} y {last}", init.join(", ")),
    }
}

/// Ejercicios con asientos en la contabilidad de TaxPain.
fn años_contables(v: &Vault) -> Result<Vec<i32>> {
    let mut stmt = v
        .connection()
        .prepare("SELECT DISTINCT fiscal_year FROM journal_entry ORDER BY 1")?;
    let rows = stmt.query_map([], |r| r.get::<_, i32>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_complementaria_cuenta_el_periodo_entero() {
        let e = Money::from_cents;
        // Complementaria: 300,00 en total; ya se ingresaron 100,00 ([70]); su [71] es 200,00.
        let b = FilingBoxes {
            casillas: [(69, e(30_000)), (70, e(10_000)), (71, e(20_000))]
                .into_iter()
                .collect(),
            ..FilingBoxes::default()
        };
        assert_eq!(resultado_303(&b), e(30_000));
        // Una declaración normal se queda como está.
        let n = FilingBoxes {
            casillas: [(71, e(-5_000))].into_iter().collect(),
            ..FilingBoxes::default()
        };
        assert_eq!(resultado_303(&n), e(-5_000));
    }

    #[test]
    fn periodos_seguidos_o_sueltos() {
        assert_eq!(periodos(&[]), "");
        assert_eq!(periodos(&["2T"]), "2T");
        assert_eq!(periodos(&["1P", "2P"]), "1P y 2P");
        assert_eq!(periodos(&["1T", "2T", "3T"]), "1T a 3T");
        assert_eq!(periodos(&["1T", "1T", "2T", "3T"]), "1T a 3T");
        assert_eq!(periodos(&["1T", "3T", "4T"]), "1T, 3T y 4T");
        assert_eq!(periodos(&["01", "02", "03", "04"]), "01 a 04");
    }
}
