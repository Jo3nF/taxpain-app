//! Datos de más para los cruces: lo que TaxPain ya sabe de la empresa (administradores en
//! Nóminas, documentos guardados, movimientos del banco, saldos de las cuentas con socios) y lo
//! que se saca de los ficheros además de lo de siempre (fecha de presentación de un modelo,
//! justificantes del Registro Mercantil).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use time::{Date, Month};

use super::super::expediente::{Clase, Doc, Expediente, TipoTramite, Tramite};
use crate::documentos::{DocKind, Facts};

/// Un administrador dado de alta en Nóminas (clave E del 190).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Administrador {
    pub nombre: String,
    pub nif: String,
}

/// Un empleado de Nóminas que no está marcado como administrador (clave A del 190), con sus
/// recibos: fecha, bruto y retención del IRPF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Empleado {
    pub nombre: String,
    pub nif: String,
    pub recibos: Vec<(Date, Money, Money)>,
}

/// Un documento de la empresa guardado en TaxPain.
#[derive(Clone, Debug)]
pub(crate) struct DocGuardado {
    pub kind: DocKind,
    pub facts: Facts,
    pub fecha: Option<Date>,
    pub fichero: String,
}

/// Un movimiento del banco (de la carpeta o de TaxPain), en euros.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MovBanco {
    pub fecha: Date,
    /// + abono, − cargo.
    pub importe: Money,
    pub concepto: String,
    /// De dónde sale: el fichero de la carpeta o «TaxPain (Bancos)».
    pub origen: String,
    /// La cuenta (la clave del extracto o la cuenta de TaxPain).
    pub cuenta: String,
}

/// Saldo al cierre de una cuenta con socios o administradores (551, 552), antes del asiento de
/// cierre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaldoSocio {
    pub ejercicio: i32,
    pub cuenta: String,
    pub nombre: String,
    /// Deudor (positivo: el socio debe a la sociedad) o acreedor (negativo).
    pub saldo: Money,
}

/// Lo que mueven en un trimestre las cuentas de IVA del diario, sin la liquidación del periodo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IvaDiario {
    /// Cargos menos abonos de la 472 (IVA soportado).
    pub soportado: Money,
    /// Abonos menos cargos de la 477 (IVA repercutido).
    pub repercutido: Money,
    /// Hay algún apunte de la 472 en el trimestre: si no, el diario no lleva el IVA soportado
    /// (las compras las lleva otro) y no se compara.
    pub con_472: bool,
    /// Hay algún apunte de la 477 en el trimestre.
    pub con_477: bool,
    /// La liquidación va dentro de un asiento con otros apuntes (un diario resumido por meses):
    /// los abonos de la 472 de ese asiento, como el IVA de una factura rectificativa recibida,
    /// no se distinguen de la liquidación y no cuentan.
    pub liquidacion_mezclada: bool,
}

/// Las retenciones que se practicaron en un trimestre según el diario: lo que se abona a la 4751
/// (H.P. acreedora por retenciones practicadas, con todas sus subcuentas), por el modelo en que
/// se declaran. Un cargo a la 4751 en un asiento sin tesorería (57x), deudas a corto (52x) ni
/// cuentas con socios (55x) no es un pago, sino una corrección, y resta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Retenciones4751 {
    /// De trabajo y de profesionales (las del 111 y el 190).
    pub trabajo_profesionales: Money,
    /// De alquileres: el asiento lleva la 621 (las del 115).
    pub alquileres: Money,
    /// Del capital mobiliario: dividendos (con la 526, la 557 o una cuenta de los grupos 11 y 12)
    /// o intereses (662, 665) (las del 123).
    pub capital: Money,
    /// De no residentes, sean del tipo que sean: lo dice el concepto del asiento, el del apunte
    /// de la 4751 o el nombre de su subcuenta («IRNR», «no residente», «modelo 216») (las del
    /// 216, art. 31 TRLIRNR).
    pub no_residentes: Money,
    /// Apuntes con un abono a la 4751 en el trimestre: un año sin ninguno no usa la 4751 (en un
    /// diario resumido, los únicos de la 4751 son los saldos de la apertura y del cierre).
    pub abonos: usize,
}

/// Lo que TaxPain ya sabe de la empresa y sirve para los cruces.
#[derive(Clone, Debug, Default)]
pub(crate) struct DatosTaxPain {
    pub administradores: Vec<Administrador>,
    /// Empleados de Nóminas que no son administradores, con sus recibos.
    pub empleados: Vec<Empleado>,
    pub documentos: Vec<DocGuardado>,
    pub movimientos: Vec<MovBanco>,
    /// Tramos con movimientos del banco en TaxPain, por cuenta: (cuenta, desde, hasta).
    pub cobertura: Vec<(String, Date, Date)>,
    pub saldos_socios: Vec<SaldoSocio>,
    /// La 472 y la 477 del diario por trimestre (año, trimestre).
    pub iva_diario: BTreeMap<(i32, u8), IvaDiario>,
    /// Las retenciones practicadas según el diario (la 4751), por trimestre (año, trimestre):
    /// todos los trimestres con asientos de los años en que el diario usa la 4751, también los
    /// que no la mueven. Un diario resumido a tres cifras (la 475 junta el IVA, las retenciones
    /// y el Impuesto sobre Sociedades) no entra: no se sabe qué son retenciones.
    pub retenciones_4751: BTreeMap<(i32, u8), Retenciones4751>,
    /// Los modelos de la gestoría guardados en TaxPain (Importar → Comparar) de los ejercicios
    /// que aún se revisan, comparados con lo que calcula TaxPain con sus datos
    /// ([`crate::presentados::compare`]). Los que no se pueden comparar no están.
    pub comparaciones: Vec<crate::presentados::Comparison>,
}

/// Lee de TaxPain lo que necesitan los cruces. Lo que no se pueda leer queda vacío: la revisión
/// sigue con lo demás.
pub(crate) fn cargar(v: &Vault, hoy: Date) -> DatosTaxPain {
    let (admins, otros): (Vec<_>, Vec<_>) = crate::payroll::employees(v)
        .unwrap_or_default()
        .into_iter()
        .partition(|e| e.administrator);
    let administradores = admins
        .into_iter()
        .map(|e| Administrador {
            nombre: e.name,
            nif: e.nif,
        })
        .collect();
    let recibos = match (
        Date::from_calendar_date(2000, Month::January, 1),
        Date::from_calendar_date(2100, Month::December, 31),
    ) {
        (Ok(a), Ok(b)) => crate::payroll::payslips(v.connection(), a, b).unwrap_or_default(),
        _ => Vec::new(),
    };
    let empleados = otros
        .into_iter()
        .map(|e| Empleado {
            recibos: recibos
                .iter()
                .filter(|p| p.employee_id == e.id && p.director_subkey.is_none())
                .map(|p| (p.date, p.gross, p.irpf))
                .collect(),
            nombre: e.name,
            nif: e.nif,
        })
        .collect();
    let documentos = crate::documentos::list(v)
        .unwrap_or_default()
        .into_iter()
        .map(|d| DocGuardado {
            kind: d.kind,
            facts: d.facts,
            fecha: d.date,
            fichero: d.filename,
        })
        .collect();
    let mut movimientos = Vec::new();
    let mut tramos: BTreeMap<String, (Date, Date)> = BTreeMap::new();
    for m in crate::bank::movements(v, false).unwrap_or_default() {
        let Some(fecha) = crate::import::parse_date(&m.op_date) else {
            continue;
        };
        if m.currency != "EUR" {
            continue;
        }
        let t = tramos
            .entry(m.bank_account.clone())
            .or_insert((fecha, fecha));
        t.0 = t.0.min(fecha);
        t.1 = t.1.max(fecha);
        movimientos.push(MovBanco {
            fecha,
            importe: m.amount,
            concepto: m.description,
            origen: "TaxPain (Bancos)".into(),
            cuenta: m.bank_account,
        });
    }
    DatosTaxPain {
        administradores,
        empleados,
        documentos,
        movimientos,
        cobertura: tramos
            .into_iter()
            .map(|(cuenta, (a, b))| (cuenta, a, b))
            .collect(),
        saldos_socios: saldos_socios(v).unwrap_or_default(),
        iva_diario: iva_diario(v).unwrap_or_default(),
        retenciones_4751: retenciones_4751(v).unwrap_or_default(),
        comparaciones: comparaciones(v, hoy),
    }
}

/// Los modelos presentados que se comparan con TaxPain, de los ejercicios que aún se revisan.
fn comparaciones(v: &Vault, hoy: Date) -> Vec<crate::presentados::Comparison> {
    let desde = super::super::revisable_desde(hoy);
    crate::presentados::list(v, None)
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.year >= desde && crate::presentados::comparable(&f.model))
        .filter_map(|f| crate::presentados::compare(v, f.id).ok())
        .collect()
}

/// Movimientos en euros de los extractos de la carpeta y de TaxPain, y los tramos de fechas que
/// cubre cada cuenta.
pub(crate) fn movimientos(exp: &Expediente) -> (Vec<MovBanco>, Vec<(String, Date, Date)>) {
    let mut movs = exp.taxpain.movimientos.clone();
    let mut tramos = exp.taxpain.cobertura.clone();
    for d in exp.docs.iter().filter(|d| d.clase == Clase::Extracto) {
        for l in &d.extractos {
            let e = &l.extracto;
            if e.divisa != "EUR" {
                continue;
            }
            let primero = e.movimientos.iter().map(|m| m.fecha).min();
            let ultimo = e.movimientos.iter().map(|m| m.fecha).max();
            if let (Some(a), Some(b)) = (e.desde.or(primero), e.hasta.or(ultimo)) {
                tramos.push((e.cuenta.clone(), a, b));
            }
            movs.extend(e.movimientos.iter().map(|m| MovBanco {
                fecha: m.fecha,
                importe: m.importe,
                concepto: m.concepto.clone(),
                origen: d.ruta.clone(),
                cuenta: e.cuenta.clone(),
            }));
        }
    }
    (movs, tramos)
}

/// La 472 y la 477 de cada trimestre con asientos, sin la liquidación del periodo, la apertura,
/// la regularización y el cierre. Los de un diario importado se conocen por su concepto y su
/// fecha («Asiento de apertura» el primer día de un mes, «Assentament de tancament» el último).
/// La liquidación puede ir en su propio asiento o, en los diarios resumidos por meses, dentro
/// del asiento del último mes del trimestre: de un asiento con la liquidación sólo cuentan los
/// cargos de la 472 y los abonos de la 477 (el IVA del mes).
pub(crate) fn iva_diario(v: &Vault) -> crate::error::Result<BTreeMap<(i32, u8), IvaDiario>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT e.id, e.date, e.concept, l.account, l.debit, l.credit
           FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.origin_kind NOT IN ('Apertura', 'Regularizacion', 'Cierre', 'LiquidacionIva')
            AND e.id IN (SELECT entry_id FROM journal_line
                          WHERE account LIKE '472%' OR account LIKE '477%')
          ORDER BY e.id",
    )?;
    type Fila = (i64, String, Option<String>, String, i64, i64);
    let filas: Vec<Fila> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut asientos: BTreeMap<i64, (String, String, Vec<Apunte>)> = BTreeMap::new();
    for (id, fecha, concepto, cuenta, debe, haber) in filas {
        asientos
            .entry(id)
            .or_insert_with(|| (fecha, concepto.unwrap_or_default(), Vec::new()))
            .2
            .push((cuenta, debe, haber));
    }
    let mut out: BTreeMap<(i32, u8), IvaDiario> = BTreeMap::new();
    for (fecha, concepto, lineas) in asientos.values() {
        let Some(fecha) = crate::import::parse_date(fecha) else {
            continue;
        };
        if apertura_o_cierre(concepto, fecha) {
            continue;
        }
        let liquida = lleva_liquidacion(lineas);
        let t = out
            .entry((fecha.year(), super::super::trimestre(fecha)))
            .or_default();
        if liquida && lineas.iter().any(|(c, ..)| !c.starts_with("47")) {
            t.liquidacion_mezclada = true;
        }
        for (cuenta, debe, haber) in lineas {
            let (debe, haber) = if liquida { (*debe, 0) } else { (*debe, *haber) };
            if cuenta.starts_with("472") && (debe, haber) != (0, 0) {
                t.soportado += Money::from_cents(debe - haber);
                t.con_472 = true;
            }
        }
        for (cuenta, debe, haber) in lineas {
            let (debe, haber) = if liquida {
                (0, *haber)
            } else {
                (*debe, *haber)
            };
            // La 4779 (IVA de otros Estados miembros, ventanilla única) va al 369, no al 303.
            if cuenta.starts_with("477")
                && !cuenta.starts_with(taxpain_core::invoice::oss::CUENTA_OSS_RAIZ)
                && (debe, haber) != (0, 0)
            {
                t.repercutido += Money::from_cents(haber - debe);
                t.con_477 = true;
            }
        }
    }
    Ok(out)
}

/// Un apunte: cuenta, debe y haber (en céntimos).
type Apunte = (String, i64, i64);

/// Las retenciones practicadas de cada trimestre según el diario (la 4751), sin la apertura, la
/// regularización ni el cierre: ver [`Retenciones4751`]. Sólo los años en que el diario usa la
/// 4751; de esos, todos los trimestres con asientos.
pub(crate) fn retenciones_4751(
    v: &Vault,
) -> crate::error::Result<BTreeMap<(i32, u8), Retenciones4751>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT id, date, concept FROM journal_entry
          WHERE origin_kind NOT IN ('Apertura', 'Regularizacion', 'Cierre')",
    )?;
    let asientos: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut trimestre_de: BTreeMap<i64, (i32, u8)> = BTreeMap::new();
    // Los textos de cada asiento que dicen si la retención es de un no residente: su concepto y
    // el concepto y el nombre de la subcuenta de sus apuntes a la 4751.
    let mut textos: BTreeMap<i64, String> = BTreeMap::new();
    for (id, fecha, concepto) in asientos {
        let Some(fecha) = crate::import::parse_date(&fecha) else {
            continue;
        };
        let concepto = concepto.unwrap_or_default();
        if apertura_o_cierre(&concepto, fecha) {
            continue;
        }
        trimestre_de.insert(id, (fecha.year(), super::super::trimestre(fecha)));
        textos.insert(id, concepto);
    }
    let mut stmt = conn.prepare(
        "SELECT l.entry_id, l.account, l.debit, l.credit, COALESCE(l.description, ''),
                COALESCE(a.name, '')
           FROM journal_line l LEFT JOIN account a ON a.code = l.account
          WHERE l.entry_id IN (SELECT entry_id FROM journal_line WHERE account LIKE '4751%')
          ORDER BY l.entry_id, l.id",
    )?;
    let filas: Vec<(i64, String, i64, i64, String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut lineas: BTreeMap<i64, Vec<Apunte>> = BTreeMap::new();
    for (id, cuenta, debe, haber, concepto, nombre) in filas {
        if cuenta.starts_with("4751")
            && let Some(t) = textos.get_mut(&id)
        {
            t.push_str(&format!(" | {concepto} | {nombre}"));
        }
        lineas.entry(id).or_default().push((cuenta, debe, haber));
    }
    let mut out: BTreeMap<(i32, u8), Retenciones4751> = BTreeMap::new();
    for (id, lineas) in &lineas {
        let Some(t) = trimestre_de.get(id) else {
            continue;
        };
        let no_residente = textos.get(id).is_some_and(|t| de_no_residentes(t));
        let r = retencion_del_asiento(lineas, no_residente);
        let x = out.entry(*t).or_default();
        x.trabajo_profesionales += r.trabajo_profesionales;
        x.alquileres += r.alquileres;
        x.capital += r.capital;
        x.no_residentes += r.no_residentes;
        x.abonos += r.abonos;
    }
    let años: std::collections::BTreeSet<i32> = out
        .iter()
        .filter(|(_, r)| r.abonos > 0)
        .map(|((y, _), _)| *y)
        .collect();
    for t in trimestre_de.values().filter(|(y, _)| años.contains(y)) {
        out.entry(*t).or_default();
    }
    out.retain(|(y, _), _| años.contains(y));
    Ok(out)
}

/// ¿Dice el texto que la retención es de un no residente? «IRNR», «no residente» (y «no
/// resident», «non residente», «ez egoiliar») o «modelo 216» («mod. 216»), por palabras
/// enteras: una «factura 216» no lo es.
fn de_no_residentes(texto: &str) -> bool {
    let palabras: Vec<String> = texto
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(crate::import::norm)
        .collect();
    palabras.iter().any(|w| w == "irnr")
        || palabras.windows(2).any(|p| {
            (matches!(p[0].as_str(), "no" | "non") && p[1].starts_with("resident"))
                || (p[0] == "ez" && p[1].starts_with("egoiliar"))
                || (matches!(p[0].as_str(), "modelo" | "mod") && p[1] == "216")
        })
}

/// Lo que un asiento abona a la 4751, por modelo, y sus abonos a la 4751. En un pago (con
/// tesorería, 57x; una póliza de crédito, 520; una cuenta corriente con socios o vinculadas,
/// 551–553, o partidas pendientes de aplicación, 555) sólo cuentan los abonos; en otro asiento,
/// los cargos son correcciones y restan. Si es de un no residente (`no_residente`), va al 216
/// sea lo que sea; si no, el modelo sale de las otras cuentas del asiento: la 621 es un
/// alquiler; la 526, la 557, los grupos 11 y 12 o la 662 y la 665, capital mobiliario.
fn retencion_del_asiento(lineas: &[Apunte], no_residente: bool) -> Retenciones4751 {
    let de_la_4751 = |c: &str| c.starts_with("4751");
    let hay = |prefijos: &[&str]| {
        lineas
            .iter()
            .any(|(c, ..)| !de_la_4751(c) && prefijos.iter().any(|p| c.starts_with(p)))
    };
    let abonos: i64 = lineas
        .iter()
        .filter(|(c, ..)| de_la_4751(c))
        .map(|(_, _, h)| h)
        .sum();
    let cargos: i64 = lineas
        .iter()
        .filter(|(c, ..)| de_la_4751(c))
        .map(|(_, d, _)| d)
        .sum();
    let pago = hay(&["57", "520", "551", "552", "553", "555"]);
    let neto = Money::from_cents(if pago { abonos } else { abonos - cargos });
    let mut r = Retenciones4751 {
        abonos: lineas
            .iter()
            .filter(|(c, _, h)| de_la_4751(c) && *h > 0)
            .count(),
        ..Retenciones4751::default()
    };
    if no_residente {
        r.no_residentes = neto;
    } else if hay(&["621"]) {
        r.alquileres = neto;
    } else if hay(&["526", "557", "11", "12", "662", "665"]) {
        r.capital = neto;
    } else {
        r.trabajo_profesionales = neto;
    }
    r
}

/// ¿Es el asiento de apertura o el de cierre de un diario importado? Por una palabra entera de
/// su concepto, en castellano, catalán y valenciano, gallego o euskera, y por la fecha: la
/// apertura el primer día de un mes y el cierre el último. Una compra a «Cierres Metálicos»
/// o una factura de mayo que diga «apertura» no lo son.
fn apertura_o_cierre(concepto: &str, fecha: Date) -> bool {
    let palabras: Vec<String> = concepto
        .split(|c: char| !c.is_alphanumeric())
        .map(crate::import::norm)
        .collect();
    let hay = |lista: &[&str]| palabras.iter().any(|w| lista.contains(&w.as_str()));
    let primero = fecha.day() == 1;
    let ultimo = fecha.day() == fecha.month().length(fecha.year());
    (primero && hay(&["apertura", "obertura", "irekiera"]))
        || (ultimo && hay(&["cierre", "tancament", "peche", "itxiera"]))
}

/// ¿Lleva el asiento la liquidación del IVA? Tiene la 4700 o la 4750 (lo que sale a devolver,
/// compensar o ingresar) o, en un diario resumido de tres cifras, la 470 o la 475 con un abono
/// de la 472 o un cargo de la 477 (la 475 lleva también las retenciones de cada mes). Si sale a
/// cero, sólo abona la 472 y carga la 477.
fn lleva_liquidacion(lineas: &[Apunte]) -> bool {
    let de = |p: &'static str| lineas.iter().filter(move |(c, ..)| c.starts_with(p));
    if lineas
        .iter()
        .any(|(c, ..)| c.starts_with("4700") || c.starts_with("4750"))
    {
        return true;
    }
    let liquida_iva =
        de("472").any(|(_, _, haber)| *haber > 0) || de("477").any(|(_, debe, _)| *debe > 0);
    if liquida_iva && lineas.iter().any(|(c, ..)| c == "470" || c == "475") {
        return true;
    }
    let solo_iva = lineas
        .iter()
        .all(|(c, ..)| c.starts_with("472") || c.starts_with("477"));
    solo_iva
        && de("472").all(|(_, debe, _)| *debe == 0)
        && de("477").all(|(_, _, haber)| *haber == 0)
}

/// Saldos al cierre de las cuentas 551 (socios y administradores) y 552 (otras personas y
/// entidades vinculadas) de cada ejercicio con asientos, sin el asiento de cierre.
fn saldos_socios(v: &Vault) -> crate::error::Result<Vec<SaldoSocio>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT e.fiscal_year, l.account, SUM(l.debit) - SUM(l.credit)
           FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
          WHERE (l.account LIKE '551%' OR l.account LIKE '552%') AND e.origin_kind <> 'Cierre'
          GROUP BY e.fiscal_year, l.account
          ORDER BY e.fiscal_year, l.account",
    )?;
    let filas: Vec<(i32, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(filas
        .into_iter()
        .filter(|(_, _, s)| *s != 0)
        .map(|(ejercicio, cuenta, saldo)| SaldoSocio {
            ejercicio,
            nombre: crate::accounting::account_name(conn, &cuenta)
                .ok()
                .flatten()
                .unwrap_or_default(),
            cuenta,
            saldo: Money::from_cents(saldo),
        })
        .collect())
}

/// Completa un documento de la carpeta con lo que sólo necesitan los cruces: la fecha de
/// presentación de un modelo en PDF y el trámite de un justificante del Registro Mercantil.
pub(crate) fn completar(d: &mut Doc, nombre: &str, bytes: &[u8]) {
    if !bytes.starts_with(b"%PDF") {
        return;
    }
    let falta_fecha = d.modelo.as_ref().is_some_and(|m| m.presentado.is_none());
    // Un documento de la Sede de la AEAT no es un justificante del Registro (los de las
    // devoluciones del Impuesto sobre Sociedades hablan del depósito de cuentas).
    let registro = d.tramite.is_none()
        && !d.empresa.as_ref().is_some_and(|(k, _)| k.de_la_aeat())
        && (matches!(d.clase, Clase::Empresa | Clase::Contable)
            || (d.clase == Clase::Otro && nombre_de_registro(nombre)));
    if !falta_fecha && !registro {
        return;
    }
    let Some(texto) = crate::pdf_text::text(bytes) else {
        return;
    };
    if falta_fecha && let Some(m) = d.modelo.as_mut() {
        // Una fecha de presentación anterior al ejercicio no es la de este modelo.
        m.presentado = fecha_de_presentacion(&texto).filter(|f| f.year() >= m.ejercicio);
    }
    if registro {
        d.tramite = tramite_de_texto(&texto);
    }
}

/// ¿El nombre del fichero suena a un justificante del Registro Mercantil?
fn nombre_de_registro(nombre: &str) -> bool {
    let n = plano(nombre);
    [
        "legaliz",
        "deposit",
        "registro",
        "mercantil",
        "cuentas anuales",
    ]
    .iter()
    .any(|p| n.contains(p))
}

/// Minúsculas y sin tildes, para buscar palabras.
fn plano(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' => 'a',
            'é' | 'è' => 'e',
            'í' | 'ì' => 'i',
            'ó' | 'ò' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            c => c,
        })
        .collect()
}

fn dia(d: &str, m: &str, y: &str) -> Option<Date> {
    let m = Month::try_from(m.parse::<u8>().ok()?).ok()?;
    Date::from_calendar_date(y.parse().ok()?, m, d.parse().ok()?).ok()
}

/// Fecha de presentación que pone un justificante («Presentación realizada el: 20-04-2025»,
/// «Fecha de presentación 20/04/2025», «Fecha de entrada…»). `TODO(VERIFY)`: comprobado con
/// textos inventados con la forma de los justificantes de la Sede de la AEAT y del Registro;
/// falta probarlo con justificantes reales.
pub(crate) fn fecha_de_presentacion(texto: &str) -> Option<Date> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?i)(?:presentaci[oó]n|fecha\s+de\s+entrada|presentad[oa])[^0-9]{0,40}(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})",
        )
        .ok()
    });
    let c = RE.as_ref()?.captures(texto)?;
    dia(&c[1], &c[2], &c[3])
}

/// Trámite del Registro Mercantil por el texto de su justificante: sólo con las marcas del
/// Registro (su nombre con la entrada, el asiento o el diario). Un informe, un correo o un
/// formulario que hablan del depósito o de la legalización no son su justificante.
pub(crate) fn tramite_de_texto(texto: &str) -> Option<Tramite> {
    static EJERCICIO: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)ejercicio\D{0,40}?(20\d{2})").ok());
    if crate::oficiales::informe_o_correo(texto)
        || !crate::oficiales::del_registro(&crate::documentos::fold(texto))
    {
        return None;
    }
    let t = plano(texto);
    let tipo = if t.contains("legalizacion") && t.contains("libro") {
        TipoTramite::LegalizacionLibros
    } else if t.contains("deposito de cuentas")
        || (t.contains("cuentas anuales") && t.contains("registro mercantil"))
    {
        TipoTramite::DepositoCuentas
    } else {
        return None;
    };
    Some(Tramite {
        tipo,
        ejercicio: EJERCICIO
            .as_ref()
            .and_then(|re| re.captures(texto))
            .and_then(|c| c[1].parse().ok()),
        presentado: fecha_de_presentacion(texto),
        resultado: None,
        cifra_negocios: None,
    })
}

/// Trámite del bloque `registro` de una lectura del asistente o de Claude Code.
pub(crate) fn tramite_de_lectura(j: &serde_json::Value, tipo: &str) -> Option<Tramite> {
    let texto = |k: &str| {
        j.get(k)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.trim().to_owned()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty() && s != "null")
    };
    let tramite = texto("tramite").map(|t| plano(&t)).unwrap_or_default();
    let tipo = if tramite.contains("legaliz") {
        TipoTramite::LegalizacionLibros
    } else if tramite.contains("deposit") || tipo == "cuentas_anuales" {
        TipoTramite::DepositoCuentas
    } else {
        return None;
    };
    Some(Tramite {
        tipo,
        ejercicio: texto("ejercicio").and_then(|y| y.parse().ok()),
        presentado: texto("presentado").and_then(|s| crate::import::parse_date(&s)),
        resultado: texto("resultado").and_then(|s| crate::received::parse_amount(&s)),
        cifra_negocios: texto("cifra_negocios").and_then(|s| crate::received::parse_amount(&s)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn apertura_y_cierre_de_un_diario_importado() {
        assert!(apertura_o_cierre(
            "Asiento de apertura",
            date!(2026 - 01 - 01)
        ));
        assert!(apertura_o_cierre(
            "Assentament d'obertura",
            date!(2026 - 01 - 01)
        ));
        assert!(apertura_o_cierre(
            "CIERRE DEL EJERCICIO 2025",
            date!(2025 - 12 - 31)
        ));
        assert!(apertura_o_cierre("Asento de peche", date!(2025 - 12 - 31)));
        // Un proveedor con la palabra en el nombre, o una fecha que no es la del cierre.
        assert!(!apertura_o_cierre(
            "Fra. 77 Cierres Metálicos",
            date!(2025 - 12 - 31)
        ));
        assert!(!apertura_o_cierre(
            "Fra. 9 Apertura de puertas",
            date!(2025 - 06 - 12)
        ));
        assert!(!apertura_o_cierre(
            "Cierre de la persiana",
            date!(2025 - 05 - 20)
        ));
    }

    /// La retención de un no residente va al 216 aunque el asiento sea un alquiler o un
    /// dividendo; el número 216 sólo cuenta detrás de «modelo».
    #[test]
    fn retenciones_de_no_residentes() {
        for si in [
            "Fra. 12 desarrollador, retención IRNR",
            "Retención no residentes",
            "Retenció no resident",
            "Retención non residente",
            "Ez egoiliarren atxikipena",
            "HP acreedora mod. 216",
        ] {
            assert!(de_no_residentes(si), "{si}");
        }
        for no in ["Factura 216 del asesor", "Retención nómina", "No procede"] {
            assert!(!de_no_residentes(no), "{no}");
        }
        let a = |c: &str, debe: i64, haber: i64| (c.to_owned(), debe, haber);
        let alquiler = [a("621", 1_000, 0), a("4751", 0, 240), a("410", 0, 760)];
        let r = retencion_del_asiento(&alquiler, true);
        assert_eq!(r.no_residentes, Money::from_cents(240));
        assert!(r.alquileres.is_zero() && r.trabajo_profesionales.is_zero());
        assert_eq!(
            retencion_del_asiento(&alquiler, false).alquileres,
            Money::from_cents(240)
        );
    }

    #[test]
    fn la_liquidacion_en_un_diario_de_tres_cifras() {
        let a = |c: &str, debe: i64, haber: i64| (c.to_owned(), debe, haber);
        // Las retenciones del mes en la 475 no son la liquidación.
        assert!(!lleva_liquidacion(&[
            a("629", 1_000, 0),
            a("472", 210, 0),
            a("475", 0, 150),
            a("410", 0, 1_060)
        ]));
        // La 475 con la 472 abonada y la 477 cargada, sí.
        assert!(lleva_liquidacion(&[
            a("477", 2_100, 0),
            a("472", 0, 210),
            a("475", 0, 1_890)
        ]));
        assert!(lleva_liquidacion(&[a("472", 0, 210), a("4700", 210, 0)]));
        // A cero: sólo la 472 abonada y la 477 cargada.
        assert!(lleva_liquidacion(&[a("477", 210, 0), a("472", 0, 210)]));
    }

    #[test]
    fn fecha_de_presentacion_del_justificante() {
        assert_eq!(
            fecha_de_presentacion("Modelo 303\nPresentación realizada el: 20-04-2025 a las 10:15"),
            Some(date!(2025 - 04 - 20))
        );
        assert_eq!(
            fecha_de_presentacion("Fecha y hora de presentación 30/01/2026 12:00"),
            Some(date!(2026 - 01 - 30))
        );
        assert_eq!(fecha_de_presentacion("Ejercicio 2025 Periodo 1T"), None);
    }

    #[test]
    fn justificantes_del_registro() {
        let t = tramite_de_texto(
            "REGISTRO MERCANTIL DE VALENCIA\nEntrada 1/2025/1234 Diario 2025 Asiento 567\n\
             Depósito de cuentas anuales del ejercicio 2024\nFecha de presentación: 28/07/2025",
        )
        .unwrap();
        assert_eq!(t.tipo, TipoTramite::DepositoCuentas);
        assert_eq!(t.ejercicio, Some(2024));
        assert_eq!(t.presentado, Some(date!(2025 - 07 - 28)));
        let t = tramite_de_texto(
            "Registro Mercantil de Valencia. Asiento 89. Legalización de libros. Ejercicio: 2024. \
             Presentado el 15-05-2025",
        )
        .unwrap();
        assert_eq!(t.tipo, TipoTramite::LegalizacionLibros);
        assert_eq!(t.presentado, Some(date!(2025 - 05 - 15)));
        assert!(tramite_de_texto("Factura de la notaría").is_none());
        // Sin las marcas del Registro, o en un informe o un correo, no hay trámite: hablan del
        // depósito o de la legalización, pero no son su justificante.
        assert!(
            tramite_de_texto("Legalización de libros. Ejercicio: 2024. Presentado el 15-05-2025")
                .is_none()
        );
        assert!(
            tramite_de_texto(
                "Revisión fiscal de Empresa Demo SL\nLegalización de libros de 2023 en el \
                 Registro Mercantil de Valencia, asiento 12, presentada el 12/11/2024"
            )
            .is_none()
        );
        assert!(
            tramite_de_texto(
                "Asunto: cuentas\nHola:\nEl depósito de cuentas de 2024 en el Registro \
                 Mercantil de Valencia (entrada 1/2025/1) se presentó el 15/09/2025."
            )
            .is_none()
        );
    }
}
