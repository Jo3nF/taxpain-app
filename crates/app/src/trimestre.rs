//! Cierre de trimestre guiado (G25): lo que conviene tener hecho antes de presentar los modelos
//! de un trimestre, con su estado y a dónde ir a arreglarlo, los borradores que tocan según el
//! perfil fiscal con su comprobación cruzada con el diario, y la liquidación del IVA.
//!
//! No hace nada por sí mismo: ordena piezas que ya existen (recibidas, banco, cartera, nóminas,
//! RETA, facturas periódicas, préstamos, borradores de `models`, cuadres de la revisión de la
//! carpeta sobre el diario propio y `accounting::settle_vat_quarter`). Lo único que guarda es lo
//! que el usuario marca a mano («hecho» o «no aplica») en los pasos que TaxPain no puede
//! comprobar del todo (tabla `cierre_trimestre_marca`).
//!
//! - IVA: autoliquidación trimestral (arts. 71 RIVA y 164 LIVA). Cuadre del 303 con el diario:
//!   devengado [27] con la 477 y total a deducir [45] con la 472, como en la revisión.
//! - Retenciones: 111 (art. 108 RIRPF), 115 y 123 frente a lo abonado a la 4751 en el trimestre.
//! - 349: entregas y servicios intracomunitarios (claves E y S) frente a la [59] del 303.
//! - «Listo para presentar» quiere decir que no queda ningún paso pendiente ni descuadre: no que
//!   los modelos estén bien. Nada se presenta desde aquí.

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::{self, Model, Period, TaxpayerProfile};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::models::{self, Draft};

/// Diferencia que se acepta en un cuadre (redondeos de las cuotas factura a factura).
const TOLERANCIA: Money = Money::from_cents(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    /// Hecho o sin nada que hacer.
    Ok,
    /// Falta algo que impide dar el trimestre por listo.
    Pendiente,
    /// Conviene mirarlo, pero no impide presentar (cobros vencidos, banco sin importar…).
    Aviso,
    /// No toca este trimestre o a esta empresa.
    NoAplica,
}

/// Lo que el usuario marca a mano en un paso.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marca {
    Hecho,
    NoAplica,
}

impl Marca {
    fn code(self) -> &'static str {
        match self {
            Marca::Hecho => "hecho",
            Marca::NoAplica => "no_aplica",
        }
    }
    fn parse(s: &str) -> Option<Marca> {
        match s {
            "hecho" => Some(Marca::Hecho),
            "no_aplica" => Some(Marca::NoAplica),
            _ => None,
        }
    }
}

/// A dónde se va a arreglar un paso: la sección y, en Contabilidad, la vista.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Destino {
    pub tab: String,
    pub view: Option<String>,
}

fn ir(tab: &str, view: Option<&str>) -> Option<Destino> {
    Some(Destino {
        tab: tab.into(),
        view: view.map(Into::into),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Paso {
    pub id: String,
    pub label: String,
    pub estado: Estado,
    pub detalle: String,
    pub destino: Option<Destino>,
    /// El usuario puede marcarlo a mano (TaxPain no lo puede comprobar del todo).
    pub manual: bool,
    /// Lo que marcó el usuario, si lo marcó (manda sobre lo calculado).
    pub marca: Option<Marca>,
}

/// Un cuadre del borrador con el diario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cuadre {
    pub concepto: String,
    pub modelo: Money,
    pub diario: Money,
    pub ok: bool,
}

/// Un modelo del trimestre según el perfil.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModeloTrimestre {
    pub model: String,
    /// Periodo como lo pide la AEAT («3T», «2P»).
    pub period: String,
    /// Positivo: a ingresar; negativo: a compensar o devolver. Cero si no hay borrador.
    pub result: Money,
    pub plazo: Option<Date>,
    pub domiciliacion: Option<Date>,
    /// Presentado (desde TaxPain, en producción, o importado de la gestoría).
    pub presentado: bool,
    pub cuadres: Vec<Cuadre>,
    pub avisos: Vec<String>,
    /// TaxPain no puede hacer el borrador: por qué.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CierreTrimestre {
    pub year: i32,
    pub quarter: u8,
    pub start: Date,
    pub end: Date,
    pub pasos: Vec<Paso>,
    pub modelos: Vec<ModeloTrimestre>,
    /// Suma de lo que sale a ingresar (sin lo ya presentado).
    pub a_ingresar: Money,
    /// Primer día límite para domiciliar de los modelos por presentar.
    pub domiciliar_hasta: Option<Date>,
    /// Ningún paso pendiente ni descuadre en los modelos por presentar.
    pub listo: bool,
    /// Todos los modelos del trimestre presentados.
    pub presentado: bool,
    /// El perfil fiscal no está configurado: los modelos salen del perfil tipo.
    pub sin_perfil: bool,
}

/// Lo que se ve de cada trimestre del año en el selector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResumenTrimestre {
    pub quarter: u8,
    pub listo: bool,
    pub presentado: bool,
    pub pendientes: usize,
    /// El trimestre aún no ha terminado.
    pub en_curso: bool,
    /// Aún no ha empezado.
    pub futuro: bool,
}

pub(crate) fn quarter_range(year: i32, quarter: u8) -> Result<(Date, Date)> {
    if !(1..=4).contains(&quarter) {
        return Err(AppError::Other(format!("trimestre {quarter} no válido")));
    }
    let first = Month::try_from(quarter * 3 - 2).map_err(|e| AppError::Other(e.to_string()))?;
    let last = Month::try_from(quarter * 3).map_err(|e| AppError::Other(e.to_string()))?;
    let start =
        Date::from_calendar_date(year, first, 1).map_err(|e| AppError::Other(e.to_string()))?;
    let end = Date::from_calendar_date(year, last, last.length(year))
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok((start, end))
}

/// Perfil fiscal guardado o, si no hay, el tipo según el titular de facturación (persona física
/// o sociedad). El segundo valor dice si es el tipo.
fn perfil(v: &Vault) -> Result<(TaxpayerProfile, bool)> {
    if let Some(p) = v.setting::<TaxpayerProfile>("taxpayer_profile")? {
        return Ok((p, false));
    }
    let persona = crate::invoicing::settings(v)?.is_none_or(|s| s.issuer.person.is_some());
    Ok((
        if persona {
            TaxpayerProfile::autonomo()
        } else {
            TaxpayerProfile::sociedad_limitada()
        },
        true,
    ))
}

/// Trimestre al que corresponde un pago fraccionado del 202 (1.º en abril por el 1T, 2.º en
/// octubre por el 3T y 3.º en diciembre, el 4T).
fn quarter_of_installment(n: u8) -> u8 {
    match n {
        1 => 1,
        2 => 3,
        _ => 4,
    }
}

/// Los modelos del trimestre según el perfil, con su periodo, su plazo y el de domiciliar.
fn modelos_del_perfil(
    p: &TaxpayerProfile,
    year: i32,
    quarter: u8,
) -> Vec<(String, u8, Option<Date>, Option<Date>)> {
    let months = [quarter * 3 - 2, quarter * 3 - 1, quarter * 3];
    let mut out: Vec<(String, u8, Option<Date>, Option<Date>)> = Vec::new();
    for y in [year, year + 1] {
        let (h, _) = calendar::calendar_for(y, p.ccaa, &p.festivos_locales);
        for d in calendar::deadlines_in_year(p, y, &h) {
            if d.fiscal_year != year {
                continue;
            }
            let (code, period) = match (d.model, d.period) {
                (
                    Model::M303
                    | Model::M111
                    | Model::M115
                    | Model::M123
                    | Model::M130
                    | Model::M131
                    | Model::M349,
                    Period::Quarter(q),
                ) if q == quarter => (d.model.code(), q),
                // Mensual (SII, gran empresa): el borrador es trimestral y avisa (`models::draft`).
                (
                    Model::M303 | Model::M111 | Model::M115 | Model::M123 | Model::M349,
                    Period::Month(m),
                ) if months.contains(&m) => (d.model.code(), quarter),
                (Model::M202, Period::Installment(n)) if quarter_of_installment(n) == quarter => {
                    ("202", n)
                }
                _ => continue,
            };
            match out.iter_mut().find(|(m, ..)| m == code) {
                // El mensual: el plazo del último mes del trimestre.
                Some(e) => {
                    if e.2.is_none_or(|x| d.due_on > x) {
                        e.2 = Some(d.due_on);
                        e.3 = d.domiciliation_end;
                    }
                }
                None => out.push((code.to_owned(), period, Some(d.due_on), d.domiciliation_end)),
            }
        }
    }
    let orden = ["303", "130", "131", "202", "111", "115", "123", "349"];
    out.sort_by_key(|(m, ..)| orden.iter().position(|o| o == m).unwrap_or(99));
    out
}

fn presentado(v: &Vault, model: &str, year: i32, period: &str) -> Result<bool> {
    Ok(v.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM tax_return WHERE model = ?1 AND year = ?2 AND period = ?3
                         AND environment = 'produccion')
             OR EXISTS(SELECT 1 FROM filed_model WHERE model = ?1 AND year = ?2 AND period = ?3)",
        params![model, year, period],
        |r| r.get(0),
    )?)
}

fn casilla(d: &Draft, n: u16) -> Money {
    d.boxes
        .iter()
        .find(|(c, _)| *c == n)
        .map_or(Money::ZERO, |(_, m)| *m)
}

fn cuadre(concepto: &str, modelo: Money, diario: Money) -> Cuadre {
    Cuadre {
        concepto: concepto.into(),
        modelo,
        diario,
        ok: (modelo - diario).abs() <= TOLERANCIA,
    }
}

/// Entregas y servicios intracomunitarios del 349 (claves E y S, sin las rectificaciones).
fn entregas_349(d: &Draft) -> Money {
    d.details
        .iter()
        .filter(|(t, _)| {
            !t.contains("rectifica") && (t.ends_with("clave E") || t.ends_with("clave S"))
        })
        .map(|(_, m)| *m)
        .sum()
}

fn marcas(v: &Vault, year: i32, quarter: u8) -> Result<Vec<(String, Marca)>> {
    let mut stmt = v.connection().prepare(
        "SELECT paso, estado FROM cierre_trimestre_marca WHERE year = ?1 AND quarter = ?2",
    )?;
    let rows = stmt.query_map(params![year, quarter], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (paso, estado) = r?;
        if let Some(m) = Marca::parse(&estado) {
            out.push((paso, m));
        }
    }
    Ok(out)
}

fn count(v: &Vault, sql: &str, p: impl taxpain_vault::rusqlite::Params) -> Result<i64> {
    Ok(v.connection().query_row(sql, p, |r| r.get(0))?)
}

fn plural(n: i64, uno: &str, varios: &str) -> String {
    if n == 1 {
        format!("1 {uno}")
    } else {
        format!("{n} {varios}")
    }
}

/// Meses del trimestre ya terminados el día `hoy` (los que deberían tener nómina o cuota).
fn meses_cumplidos(start: Date, end: Date, hoy: Date) -> Vec<(i32, u8)> {
    let mut out = Vec::new();
    let mut d = start;
    while d <= end {
        let last =
            Date::from_calendar_date(d.year(), d.month(), d.month().length(d.year())).unwrap_or(d);
        if last <= hoy {
            out.push((d.year(), u8::from(d.month())));
        }
        d = last.next_day().unwrap_or(last);
        if d == last {
            break;
        }
    }
    out
}

/// Meses (de `meses`) sin ninguna fila de `tabla` (payslip o reta_cuota) del empleado.
fn meses_sin(v: &Vault, tabla: &str, employee: i64, meses: &[(i32, u8)]) -> Result<usize> {
    let mut faltan = 0;
    for (y, m) in meses {
        let n = count(
            v,
            &format!(
                "SELECT count(*) FROM {tabla} WHERE employee_id = ?1 AND substr(date, 1, 7) = ?2"
            ),
            params![employee, format!("{y:04}-{m:02}")],
        )?;
        if n == 0 {
            faltan += 1;
        }
    }
    Ok(faltan)
}

/// Los pasos del trimestre (sin los modelos).
fn pasos(
    v: &Vault,
    year: i32,
    (start, end): (Date, Date),
    hoy: Date,
    p: &TaxpayerProfile,
) -> Result<Vec<Paso>> {
    let (s, e) = (start.to_string(), end.to_string());
    let mut out: Vec<Paso> = Vec::new();
    let mut push = |id: &str,
                    label: &str,
                    estado: Estado,
                    detalle: String,
                    destino: Option<Destino>,
                    manual: bool| {
        out.push(Paso {
            id: id.into(),
            label: label.into(),
            estado,
            detalle,
            destino,
            manual,
            marca: None,
        });
    };

    // 1. Recibidas por revisar: sin revisar no cuentan en el 303 ni en el diario.
    let recibidas = count(
        v,
        "SELECT count(*) FROM invoice_received WHERE status = 'pendiente'
           AND (issue_date IS NULL OR issue_date BETWEEN ?1 AND ?2)",
        params![s, e],
    )?;
    push(
        "recibidas",
        "Facturas recibidas revisadas",
        if recibidas == 0 {
            Estado::Ok
        } else {
            Estado::Pendiente
        },
        if recibidas == 0 {
            "Todas las del trimestre están revisadas.".into()
        } else {
            format!(
                "Quedan {} del trimestre (o sin fecha): sin revisar no cuentan en el 303 ni en el \
                 diario.",
                plural(recibidas, "por revisar", "por revisar")
            )
        },
        ir("recibidas", None),
        false,
    );

    // 2. Emitidas vencidas sin cobrar: no impiden presentar (el IVA se devenga igual, art. 75
    // LIVA), pero conviene reclamarlas.
    let corte = if hoy < end { hoy } else { end };
    let vencidas: Vec<_> = crate::cartera::list(v, hoy)?
        .into_iter()
        .filter(|d| d.kind == "emitida" && d.status == "pendiente" && d.due_date < corte)
        .collect();
    let importe: Money = vencidas.iter().map(|d| d.amount).sum();
    push(
        "cobros",
        "Facturas emitidas vencidas cobradas",
        if vencidas.is_empty() {
            Estado::Ok
        } else {
            Estado::Aviso
        },
        if vencidas.is_empty() {
            "Ningún cobro vencido pendiente.".into()
        } else {
            format!(
                "{} sin cobrar ({importe}). El IVA se declara igual aunque no se haya cobrado; si \
                 ya se cobró, concílialo con el banco.",
                plural(
                    vencidas.len() as i64,
                    "factura vencida",
                    "facturas vencidas"
                )
            )
        },
        ir("contabilidad", Some("cartera")),
        true,
    );

    // 3. Banco conciliado (como el cierre anual).
    let movimientos = count(
        v,
        "SELECT count(*) FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
          WHERE t.op_date BETWEEN ?1 AND ?2 AND b.kind <> 'tarjeta_debito'",
        params![s, e],
    )?;
    let sin_conciliar = count(
        v,
        &format!(
            "SELECT count(*) FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
             WHERE t.entry_id IS NULL AND t.op_date BETWEEN ?1 AND ?2
               AND b.kind <> 'tarjeta_debito' AND NOT {}",
            crate::bank::BOOKED_ELSEWHERE
        ),
        params![s, e],
    )?;
    push(
        "banco",
        "Banco conciliado",
        if movimientos == 0 {
            Estado::Aviso
        } else if sin_conciliar == 0 {
            Estado::Ok
        } else {
            Estado::Pendiente
        },
        if movimientos == 0 {
            "No hay movimientos del banco del trimestre: importa el extracto (o conéctalo) para \
             ver si falta alguna factura."
                .into()
        } else if sin_conciliar == 0 {
            "Todos los movimientos del trimestre tienen asiento.".into()
        } else {
            format!(
                "{}.",
                plural(
                    sin_conciliar,
                    "movimiento sin conciliar",
                    "movimientos sin conciliar"
                )
            )
        },
        ir("contabilidad", Some("banco")),
        movimientos == 0,
    );

    // 4. Nóminas y 5. cuotas del RETA de los meses ya terminados.
    let meses = meses_cumplidos(start, end, hoy);
    let empleados: Vec<_> = crate::payroll::employees(v)?
        .into_iter()
        .filter(|e| e.active)
        .collect();
    let con_nomina: Vec<_> = empleados
        .iter()
        .filter(|e| {
            count(
                v,
                "SELECT count(*) FROM payslip WHERE employee_id = ?1",
                [e.id],
            )
            .unwrap_or(0)
                > 0
                || !e.administrator
        })
        .collect();
    if con_nomina.is_empty() && !p.tiene_trabajadores {
        push(
            "nominas",
            "Nóminas del trimestre",
            Estado::NoAplica,
            "Sin empleados en Nóminas.".into(),
            ir("contabilidad", Some("nominas")),
            true,
        );
    } else {
        let mut faltan = Vec::new();
        for e in &con_nomina {
            let n = meses_sin(v, "payslip", e.id, &meses)?;
            if n > 0 {
                faltan.push(format!(
                    "{} ({n} {})",
                    e.name,
                    if n == 1 { "mes" } else { "meses" }
                ));
            }
        }
        push(
            "nominas",
            "Nóminas del trimestre",
            if faltan.is_empty() && !con_nomina.is_empty() {
                Estado::Ok
            } else {
                Estado::Pendiente
            },
            if con_nomina.is_empty() {
                "El perfil dice que tienes trabajadores, pero no hay ninguno en Nóminas: si las \
                 lleva una gestoría laboral, contabiliza sus recibos y márcalo como hecho."
                    .into()
            } else if faltan.is_empty() {
                "Hay recibo de cada mes terminado.".into()
            } else {
                format!(
                    "Sin recibo de algún mes terminado: {}. Sin la nómina, sus retenciones no \
                     salen en el 111.",
                    faltan.join(", ")
                )
            },
            ir("contabilidad", Some("nominas")),
            true,
        );
    }
    let fichas: Vec<(i64, String)> = {
        let mut stmt = v.connection().prepare(
            "SELECT f.employee_id, e.name FROM reta_ficha f JOIN employee e ON e.id = f.employee_id
              WHERE f.year = ?1 AND f.pagador = 'Sociedad' AND e.active = 1",
        )?;
        stmt.query_map([year], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    if fichas.is_empty() && !p.cotiza_reta {
        push(
            "reta",
            "Cuotas del RETA",
            Estado::NoAplica,
            "Ninguna cuota de autónomos que pague la empresa.".into(),
            ir("contabilidad", Some("nominas")),
            true,
        );
    } else {
        let mut faltan = Vec::new();
        for (id, name) in &fichas {
            let n = meses_sin(v, "reta_cuota", *id, &meses)?;
            if n > 0 {
                faltan.push(format!(
                    "{name} ({n} {})",
                    if n == 1 { "mes" } else { "meses" }
                ));
            }
        }
        push(
            "reta",
            "Cuotas del RETA",
            if fichas.is_empty() {
                Estado::Aviso
            } else if faltan.is_empty() {
                Estado::Ok
            } else {
                Estado::Pendiente
            },
            if fichas.is_empty() {
                "Cotizas al RETA: si la cuota la paga la empresa, regístrala cada mes en Nóminas \
                 (autónomo societario); si la pagas tú, márcalo como no aplica."
                    .into()
            } else if faltan.is_empty() {
                "Registrada la cuota de cada mes terminado.".into()
            } else {
                format!(
                    "Sin la cuota de algún mes terminado: {}.",
                    faltan.join(", ")
                )
            },
            ir("contabilidad", Some("nominas")),
            true,
        );
    }

    // 6. Facturas periódicas que tocaban en el trimestre y no se han emitido.
    let periodicas = crate::recurring::list(v, hoy)?
        .into_iter()
        .filter(|t| t.next_date.is_some_and(|d| d <= end && d <= hoy))
        .count();
    push(
        "periodicas",
        "Facturas periódicas emitidas",
        if periodicas == 0 {
            Estado::Ok
        } else {
            Estado::Pendiente
        },
        if periodicas == 0 {
            "Ninguna plantilla con una factura por emitir del trimestre.".into()
        } else {
            format!(
                "{} con una factura del trimestre por emitir (Facturas → Periódicas).",
                plural(periodicas as i64, "plantilla", "plantillas")
            )
        },
        ir("facturas", None),
        true,
    );

    // 7. Amortizaciones: TaxPain las contabiliza una vez al año, en el cierre del ejercicio.
    let fy = crate::accounting::fiscal_year(v, year).ok();
    let cierra = fy.as_ref().is_some_and(|f| f.end >= start && f.end <= end);
    if cierra {
        let falta = crate::accounting::accrued_depreciation(v, year, end)?;
        push(
            "amortizaciones",
            "Amortizaciones del ejercicio",
            if falta.is_zero() {
                Estado::Ok
            } else {
                Estado::Pendiente
            },
            if falta.is_zero() {
                "Contabilizadas.".into()
            } else {
                format!(
                    "Faltan {falta}: el trimestre cierra el ejercicio (Contabilidad → \
                     Inmovilizado → «Amortizar»)."
                )
            },
            ir("contabilidad", Some("inmovilizado")),
            false,
        );
    } else {
        push(
            "amortizaciones",
            "Amortizaciones",
            Estado::NoAplica,
            "TaxPain las contabiliza una vez al año, al cierre del ejercicio; no cambian los \
             modelos del trimestre."
                .into(),
            ir("contabilidad", Some("inmovilizado")),
            false,
        );
    }

    // 8. Cuotas de los préstamos vencidas en el trimestre sin registrar.
    let prestamos = crate::prestamos::list(v, hoy)?;
    let cuotas: Vec<String> = prestamos
        .iter()
        .flat_map(|pv| {
            pv.cuotas
                .iter()
                .filter(|c| {
                    c.estado == crate::prestamos::EstadoCuota::Pendiente
                        && c.q.fecha >= start
                        && c.q.fecha <= end
                        && c.q.fecha <= hoy
                })
                .map(|c| format!("{} (cuota {})", pv.prestamo.nombre, c.q.n))
        })
        .collect();
    push(
        "prestamos",
        "Cuotas de los préstamos",
        if prestamos.is_empty() {
            Estado::NoAplica
        } else if cuotas.is_empty() {
            Estado::Ok
        } else {
            Estado::Pendiente
        },
        if prestamos.is_empty() {
            "Sin préstamos en TaxPain.".into()
        } else if cuotas.is_empty() {
            "Registradas las cuotas vencidas del trimestre.".into()
        } else {
            format!(
                "Sin registrar: {}. Sus intereses y, si los hay, su retención (123) dependen de \
                 ello.",
                cuotas.join(", ")
            )
        },
        ir("contabilidad", Some("prestamos")),
        true,
    );

    // 9. VERI*FACTU enviado (lo aceptado sólo en pruebas no está remitido).
    let sin_enviar = count(
        v,
        "SELECT count(*) FROM verifactu_record r JOIN invoice_issued i ON i.id = r.invoice_id
          WHERE r.send_status IN ('pendiente', 'pruebas') AND i.issue_date BETWEEN ?1 AND ?2",
        params![s, e],
    )
    .unwrap_or(0);
    push(
        "verifactu",
        "Registros VERI*FACTU enviados",
        if sin_enviar == 0 {
            Estado::Ok
        } else {
            Estado::Pendiente
        },
        if sin_enviar == 0 {
            "No queda ninguno del trimestre por enviar.".into()
        } else {
            format!(
                "{} del trimestre sin remitir a la AEAT (Facturas).",
                plural(sin_enviar, "registro", "registros")
            )
        },
        ir("facturas", None),
        false,
    );
    Ok(out)
}

/// Paso de la liquidación del IVA (después de los modelos).
fn paso_liquidacion(v: &Vault, year: i32, quarter: u8, iva: bool) -> Result<Paso> {
    let key = i64::from(year) * 100 + i64::from(quarter);
    let hecho = v
        .connection()
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'LiquidacionIva' AND origin_id = ?1",
            [key],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    Ok(Paso {
        id: "liquidacion_iva".into(),
        label: "Liquidación del IVA contabilizada".into(),
        estado: match (iva, hecho) {
            (false, _) => Estado::NoAplica,
            (true, true) => Estado::Ok,
            (true, false) => Estado::Pendiente,
        },
        detalle: match (iva, hecho) {
            (false, _) => "Sin IVA periódico en el perfil.".into(),
            (true, true) => {
                "Hay asiento de liquidación del trimestre (477/472 a la 4750 o 4700).".into()
            }
            (true, false) => "Cuando el 303 esté bien, contabiliza su liquidación (Contabilidad → \
                              IVA y libros): salda la 472 y la 477 del trimestre."
                .into(),
        },
        destino: ir("contabilidad", Some("impuestos")),
        manual: false,
        marca: None,
    })
}

fn modelo(
    v: &Vault,
    year: i32,
    (model, period_n, plazo, domiciliacion): (String, u8, Option<Date>, Option<Date>),
    q: u8,
    cache303: &mut Option<Option<Draft>>,
) -> Result<ModeloTrimestre> {
    let period = models::period_code(&model, period_n);
    let filed = presentado(v, &model, year, &period)?;
    let mut m = ModeloTrimestre {
        model: model.clone(),
        period,
        result: Money::ZERO,
        plazo,
        domiciliacion,
        presentado: filed,
        cuadres: Vec::new(),
        avisos: Vec::new(),
        error: None,
    };
    let draft = match models::draft(v, &model, year, period_n) {
        Ok(d) => d,
        Err(e) => {
            m.error = Some(match &e {
                AppError::NotConfigured => "Configura antes tus datos de facturación.".into(),
                _ if model == "131" => "TaxPain no hace el borrador del 131 (estimación \
                                        objetiva): calcúlalo en la Sede."
                    .into(),
                _ => e.to_string(),
            });
            return Ok(m);
        }
    };
    m.result = draft.result;
    m.avisos = draft.warnings.clone();
    match model.as_str() {
        "303" => {
            if let Some(x) = crate::revision::cruces::datos::iva_diario(v)?
                .get(&(year, q))
                .copied()
            {
                if x.con_477 {
                    m.cuadres.push(cuadre(
                        "IVA devengado [27] con la 477 del diario",
                        casilla(&draft, 27),
                        x.repercutido,
                    ));
                }
                if x.con_472 {
                    m.cuadres.push(cuadre(
                        "IVA deducible [45] con la 472 del diario",
                        casilla(&draft, 45),
                        x.soportado,
                    ));
                }
            }
            *cache303 = Some(Some(draft));
        }
        "111" | "115" | "123" => {
            if let Some(r) = crate::revision::cruces::datos::retenciones_4751(v)?
                .get(&(year, q))
                .copied()
            {
                let (concepto, diario, modelo) = match model.as_str() {
                    "111" => (
                        "Retenciones [28] con la 4751 del diario (trabajo y profesionales)",
                        r.trabajo_profesionales,
                        if casilla(&draft, 28).is_zero() {
                            draft.result
                        } else {
                            casilla(&draft, 28)
                        },
                    ),
                    "115" => (
                        "Retenciones con la 4751 del diario (alquileres)",
                        r.alquileres,
                        draft.result,
                    ),
                    _ => (
                        "Retenciones con la 4751 del diario (capital mobiliario)",
                        r.capital,
                        draft.result,
                    ),
                };
                if r.abonos > 0 || !modelo.is_zero() {
                    m.cuadres.push(cuadre(concepto, modelo, diario));
                }
            }
        }
        "349" => {
            if cache303.is_none() {
                *cache303 = Some(models::draft(v, "303", year, q).ok());
            }
            if let Some(Some(d303)) = cache303.as_ref() {
                let e = entregas_349(&draft);
                let c59 = casilla(d303, 59);
                if !e.is_zero() || !c59.is_zero() {
                    m.cuadres.push(cuadre(
                        "Entregas y servicios intracomunitarios (claves E y S) con la [59] del 303",
                        e,
                        c59,
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(m)
}

/// El cierre del trimestre `quarter` (1–4) de `year` a día `hoy`.
pub fn cierre(v: &Vault, year: i32, quarter: u8, hoy: Date) -> Result<CierreTrimestre> {
    let rango = quarter_range(year, quarter)?;
    let (p, sin_perfil) = perfil(v)?;
    let mut pasos = pasos(v, year, rango, hoy, &p)?;
    let mut cache303 = None;
    let mut modelos = Vec::new();
    for x in modelos_del_perfil(&p, year, quarter) {
        modelos.push(modelo(v, year, x, quarter, &mut cache303)?);
    }
    pasos.push(paso_liquidacion(v, year, quarter, p.iva.is_some())?);
    for (id, m) in marcas(v, year, quarter)? {
        if let Some(paso) = pasos.iter_mut().find(|x| x.id == id && x.manual) {
            paso.marca = Some(m);
            paso.estado = match m {
                Marca::Hecho => Estado::Ok,
                Marca::NoAplica => Estado::NoAplica,
            };
        }
    }
    let por_presentar: Vec<&ModeloTrimestre> = modelos.iter().filter(|m| !m.presentado).collect();
    let a_ingresar = por_presentar
        .iter()
        .map(|m| m.result)
        .filter(|r| !r.is_negative())
        .sum();
    let domiciliar_hasta = por_presentar
        .iter()
        .filter(|m| m.result.cents() > 0)
        .filter_map(|m| m.domiciliacion)
        .min();
    // La liquidación del IVA va después de presentar: no impide estar listo.
    let listo = pasos
        .iter()
        .filter(|x| x.id != "liquidacion_iva")
        .all(|x| x.estado != Estado::Pendiente)
        && por_presentar
            .iter()
            .all(|m| m.error.is_none() && m.cuadres.iter().all(|c| c.ok));
    let presentado = !modelos.is_empty() && por_presentar.is_empty();
    Ok(CierreTrimestre {
        year,
        quarter,
        start: rango.0,
        end: rango.1,
        pasos,
        modelos,
        a_ingresar,
        domiciliar_hasta,
        listo,
        presentado,
        sin_perfil,
    })
}

/// Los cuatro trimestres de `year`: si están listos, presentados y cuántos pasos quedan.
pub fn resumen(v: &Vault, year: i32, hoy: Date) -> Result<Vec<ResumenTrimestre>> {
    (1..=4)
        .map(|q| {
            let (start, end) = quarter_range(year, q)?;
            if start > hoy {
                return Ok(ResumenTrimestre {
                    quarter: q,
                    listo: false,
                    presentado: false,
                    pendientes: 0,
                    en_curso: false,
                    futuro: true,
                });
            }
            let c = cierre(v, year, q, hoy)?;
            Ok(ResumenTrimestre {
                quarter: q,
                listo: c.listo,
                presentado: c.presentado,
                pendientes: c
                    .pasos
                    .iter()
                    .filter(|x| x.estado == Estado::Pendiente)
                    .count()
                    + c.modelos
                        .iter()
                        .filter(|m| !m.presentado && m.cuadres.iter().any(|x| !x.ok))
                        .count(),
                en_curso: end >= hoy,
                futuro: false,
            })
        })
        .collect()
}

/// Pasos que el usuario puede marcar a mano.
const MANUALES: [&str; 6] = [
    "cobros",
    "banco",
    "nominas",
    "reta",
    "periodicas",
    "prestamos",
];

/// Marca (o desmarca, con `None`) a mano un paso del trimestre. Queda en la auditoría.
pub fn marcar(
    v: &Vault,
    year: i32,
    quarter: u8,
    paso: &str,
    marca: Option<Marca>,
    now: OffsetDateTime,
) -> Result<()> {
    quarter_range(year, quarter)?;
    if !MANUALES.contains(&paso) {
        return Err(AppError::Other(format!(
            "el paso «{paso}» no se marca a mano: TaxPain lo comprueba solo"
        )));
    }
    let conn = v.connection();
    match marca {
        Some(m) => conn.execute(
            "INSERT INTO cierre_trimestre_marca (year, quarter, paso, estado, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(year, quarter, paso) DO UPDATE SET estado = ?4, updated_at = ?5",
            params![year, quarter, paso, m.code(), now_str(now)],
        )?,
        None => conn.execute(
            "DELETE FROM cierre_trimestre_marca WHERE year = ?1 AND quarter = ?2 AND paso = ?3",
            params![year, quarter, paso],
        )?,
    };
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "quarter_close.mark",
        &format!("{year}-{quarter}T"),
        &serde_json::json!({ "paso": paso, "marca": marca.map(Marca::code) }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn meses_terminados_del_trimestre() {
        let (s, e) = quarter_range(2026, 3).unwrap();
        assert_eq!(
            meses_cumplidos(s, e, date!(2026 - 08 - 15)),
            vec![(2026, 7)]
        );
        assert_eq!(meses_cumplidos(s, e, date!(2026 - 09 - 30)).len(), 3);
        assert!(meses_cumplidos(s, e, date!(2026 - 07 - 30)).is_empty());
    }

    #[test]
    fn modelos_de_una_sl_por_trimestre() {
        let p = TaxpayerProfile::sociedad_limitada();
        let q3: Vec<String> = modelos_del_perfil(&p, 2026, 3)
            .into_iter()
            .map(|(m, n, ..)| format!("{m}-{n}"))
            .collect();
        // 202: el 2.º pago fraccionado (octubre) va con el 3T.
        assert_eq!(q3, ["303-3", "202-2", "111-3", "115-3"]);
        let q2: Vec<String> = modelos_del_perfil(&p, 2026, 2)
            .into_iter()
            .map(|(m, ..)| m)
            .collect();
        assert_eq!(q2, ["303", "111", "115"]);
        let a = TaxpayerProfile::autonomo();
        let q4: Vec<String> = modelos_del_perfil(&a, 2026, 4)
            .into_iter()
            .map(|(m, ..)| m)
            .collect();
        assert_eq!(q4, ["303", "130"]);
    }

    #[test]
    fn entregas_intracomunitarias_del_349() {
        let d = Draft {
            model: "349".into(),
            year: 2026,
            quarter: 1,
            boxes: vec![],
            payees: vec![],
            details: vec![
                (
                    "Cliente UE · FR123 · clave S".into(),
                    Money::from_cents(100_000),
                ),
                (
                    "Proveedor UE · DE1 · clave I".into(),
                    Money::from_cents(50_000),
                ),
                (
                    "Cliente UE · FR123 · clave S · rectifica 4T 2025 (antes 1,00 €)".into(),
                    Money::from_cents(900),
                ),
            ],
            declaration: String::new(),
            result: Money::ZERO,
            warnings: vec![],
            file_name: String::new(),
            file: String::new(),
            terceros: vec![],
        };
        assert_eq!(entregas_349(&d), Money::from_cents(100_000));
    }
}
