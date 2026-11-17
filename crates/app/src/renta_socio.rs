//! Datos para la Renta y el Patrimonio del socio o administrador (G34): lo que la sociedad sabe
//! de una persona en un año y le sirve para su declaración. **No calcula su IRPF ni su Impuesto
//! sobre el Patrimonio, ni le aconseja**: junta las cifras con su origen y la casilla en la que
//! suelen ir, para que lo que declare cuadre con lo que la sociedad declara en el 190, el 193 y
//! el 123.
//!
//! - Retribuciones del trabajo y del cargo de administrador (nóminas, clave A o E del 190) con
//!   su retención (art. 17.1 y 17.2.e LIRPF; art. 101.2 LIRPF).
//! - Retribución en especie: la cuota del RETA que paga la sociedad, con su ingreso a cuenta
//!   (art. 42 y 43 LIRPF; `autonomo_societario`), y la de los préstamos de la sociedad a un
//!   interés inferior al legal del dinero (art. 43.1.1.º.c LIRPF), estimada.
//! - Cotizaciones a la Seguridad Social que el socio puede restar (art. 19.2.a LIRPF).
//! - Dividendos cobrados y su retención (art. 25.1.a LIRPF; 193).
//! - Intereses de los préstamos del socio a la sociedad (art. 25.2 LIRPF; 123 y 193), con la
//!   presunción de retribución si no hay interés (art. 6.5 LIRPF) y el aviso del exceso sobre el
//!   triple de los fondos propios (art. 46.a LIRPF).
//! - Facturas que el socio emite a la sociedad (alquiler o servicios) con su retención.
//! - Impuesto sobre el Patrimonio: valor de las participaciones no cotizadas por el valor teórico
//!   del último balance aprobado (art. 16.Uno LIP) y semáforo de la exención de empresa familiar
//!   (art. 4.Ocho.Dos LIP), con el cálculo de entidad patrimonial de `patrimonial` (más de 90
//!   días con más de la mitad del activo en valores o no afecto, sin lo comprado con beneficios
//!   de actividad de los diez años anteriores).
//!
//! `TODO(VERIFY)`: los números de casilla son los de Renta WEB de los últimos ejercicios y la
//! AEAT puede cambiarlos cada año: el informe lo dice y hay que cotejarlos con el borrador.

use serde::Serialize;
use taxpain_core::accounting::prestamo::{self as calc, Sentido};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;
use time::{Date, Month};

use crate::autonomo_societario::{self, Pagador};
use crate::error::{AppError, Result};
use crate::{accounting, companies, patrimonial, payroll, prestamos, socios};

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn norm(nif: &str) -> String {
    nif.trim().to_uppercase()
}

/// Una persona de la que la sociedad tiene datos (socio, administrador o empleado).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Persona {
    pub nif: String,
    pub nombre: String,
    pub socio: bool,
    pub administrador: bool,
    /// Participación en millonésimas (100 % = 1.000.000).
    pub pct: u32,
}

/// Socios personas físicas y administradores (fichas de Socios y de Nóminas), sin repetir NIF.
pub fn personas(v: &Vault) -> Result<Vec<Persona>> {
    let mut out: Vec<Persona> = Vec::new();
    for s in socios::shareholders(v)? {
        if s.legal_person || !s.active {
            continue;
        }
        out.push(Persona {
            nif: norm(&s.nif),
            nombre: s.name.clone(),
            socio: true,
            administrador: false,
            pct: s.pct,
        });
    }
    for e in payroll::employees(v)? {
        if !e.administrator {
            continue;
        }
        let nif = norm(&e.nif);
        if let Some(p) = out.iter_mut().find(|p| p.nif == nif) {
            p.administrador = true;
        } else {
            out.push(Persona {
                nif,
                nombre: e.name.clone(),
                socio: false,
                administrador: true,
                pct: 0,
            });
        }
    }
    out.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    Ok(out)
}

/// Un concepto con su importe, la retención o el ingreso a cuenta y dónde va.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Concepto {
    /// Apartado de la declaración («Rendimientos del trabajo», «Capital mobiliario»…).
    pub apartado: String,
    pub concepto: String,
    /// Casilla de Renta WEB (orientativa: `TODO(VERIFY)` cada ejercicio).
    pub casilla: String,
    pub importe: Money,
    /// Retención o ingreso a cuenta.
    pub retencion: Money,
    /// De dónde sale (pantalla o modelo de la sociedad).
    pub fuente: String,
    #[serde(default)]
    pub nota: String,
    /// Estimación de TaxPain (no sale de un modelo presentado ni de un asiento).
    pub estimado: bool,
}

/// Estado de un requisito de la exención.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Semaforo {
    Cumple,
    Dudoso,
    NoCumple,
    SinDatos,
}

impl Semaforo {
    pub fn texto(self) -> &'static str {
        match self {
            Semaforo::Cumple => "parece que se cumple",
            Semaforo::Dudoso => "dudoso: revísalo",
            Semaforo::NoCumple => "parece que no se cumple",
            Semaforo::SinDatos => "faltan datos",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Requisito {
    pub titulo: String,
    pub estado: Semaforo,
    pub detalle: String,
    pub norma: String,
}

/// Impuesto sobre el Patrimonio: valor de las participaciones y exención.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PatrimonioSocio {
    /// Ejercicio del balance que se toma como el último aprobado al 31 de diciembre.
    pub ejercicio_balance: i32,
    pub fecha_balance: Option<Date>,
    /// Patrimonio neto contable de ese balance (`None`: no hay contabilidad de ese ejercicio).
    pub patrimonio_neto: Option<Money>,
    /// Participación en millonésimas.
    pub pct: u32,
    /// Valor teórico de sus participaciones (patrimonio neto × participación; nunca negativo).
    pub valor_teorico: Option<Money>,
    pub requisitos: Vec<Requisito>,
    pub exencion: Semaforo,
    /// Parte del activo afecta a la actividad (media de los balances trimestrales del año):
    /// orientación de la parte exenta (art. 4.Ocho.Dos, último párrafo, LIP).
    pub proporcion_afecta: Option<Rate>,
    /// Valor teórico × proporción afecta (orientativo, sólo si la exención no está descartada).
    pub valor_exento_orientativo: Option<Money>,
    pub notas: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InformeSocio {
    pub year: i32,
    pub persona: Persona,
    pub empresa: String,
    pub nif_empresa: String,
    pub conceptos: Vec<Concepto>,
    pub patrimonio: PatrimonioSocio,
    pub avisos: Vec<String>,
    pub notas: Vec<String>,
}

/// El año natural del IRPF y del Patrimonio (devengo el 31 de diciembre: art. 29 LIP).
fn bounds(year: i32) -> Result<(Date, Date)> {
    Ok((
        Date::from_calendar_date(year, Month::January, 1).map_err(|e| other(e.to_string()))?,
        Date::from_calendar_date(year, Month::December, 31).map_err(|e| other(e.to_string()))?,
    ))
}

fn ratio(num: Money, den: Money) -> Rate {
    if den.cents() <= 0 {
        return Rate::ZERO;
    }
    let bp = i128::from(num.cents()) * 10_000 / i128::from(den.cents());
    Rate::from_bp(i32::try_from(bp.clamp(0, 10_000)).unwrap_or(0))
}

fn por_participacion(m: Money, pct: u32) -> Money {
    let c = i128::from(m.cents()) * i128::from(pct);
    Money::from_cents(i64::try_from((c + 500_000) / 1_000_000).unwrap_or(0))
}

fn pct_txt(pct: u32) -> String {
    // Millonésimas → centésimas de punto, redondeadas.
    let bp = (pct + 50) / 100;
    format!("{},{:02} %", bp / 100, bp % 100)
}

fn concepto(
    apartado: &str,
    concepto: impl Into<String>,
    casilla: &str,
    importe: Money,
    retencion: Money,
    fuente: &str,
) -> Concepto {
    Concepto {
        apartado: apartado.into(),
        concepto: concepto.into(),
        casilla: casilla.into(),
        importe,
        retencion,
        fuente: fuente.into(),
        nota: String::new(),
        estimado: false,
    }
}

const TRABAJO: &str = "Rendimientos del trabajo";
const CAPITAL: &str = "Rendimientos del capital mobiliario";
const OTROS: &str = "Otros (según su naturaleza)";

/// Intereses al tipo `tipo` de un capital `capital` durante `dias` de un año de `base` días.
fn interes(capital: Money, tipo: Rate, dias: i64, base: i64) -> Money {
    let anual = capital.apply_rate(tipo);
    let c = i128::from(anual.cents()) * i128::from(dias.max(0));
    Money::from_cents(i64::try_from(c / i128::from(base.max(1))).unwrap_or(0))
}

/// Informe del año para la persona con NIF `nif`. `otros_rendimientos`: sus rendimientos netos
/// del trabajo y de actividades que no vienen de esta sociedad (para el requisito de la
/// remuneración de la exención; `None` si no se sabe).
pub fn informe(
    v: &Vault,
    year: i32,
    nif: &str,
    otros_rendimientos: Option<Money>,
    hoy: Date,
) -> Result<InformeSocio> {
    let nif = norm(nif);
    let persona = personas(v)?
        .into_iter()
        .find(|p| p.nif == nif)
        .ok_or_else(|| other("no hay ningún socio persona física ni administrador con ese NIF"))?;
    let info = companies::info(v)?;
    let (start, end) = bounds(year)?;
    let mut conceptos = Vec::new();
    let mut avisos = Vec::new();

    // Nóminas (190).
    let empleado = payroll::employees(v)?
        .into_iter()
        .find(|e| norm(&e.nif) == nif);
    let mut retribucion = Money::ZERO;
    if let Some(e) = &empleado {
        let nominas: Vec<_> = payroll::payslips(v.connection(), start, end)?
            .into_iter()
            .filter(|s| s.employee_id == e.id)
            .collect();
        let bruto: Money = nominas.iter().map(|s| s.gross).sum();
        let irpf: Money = nominas.iter().map(|s| s.irpf).sum();
        let ss: Money = nominas.iter().map(|s| s.ss_employee).sum();
        retribucion += bruto;
        if !bruto.is_zero() {
            let clave = match nominas.iter().find_map(|s| s.director_subkey) {
                Some(k) => format!("190, clave E, subclave {k:02}"),
                None if e.administrator => "190, clave E".to_owned(),
                None => "190, clave A".to_owned(),
            };
            let mut c = concepto(
                TRABAJO,
                if e.administrator {
                    "Retribuciones dinerarias (cargo de administrador y trabajo)"
                } else {
                    "Retribuciones dinerarias"
                },
                "0001",
                bruto,
                irpf,
                &format!("{} nóminas · {clave}", nominas.len()),
            );
            c.nota = "La retención va en la casilla de retenciones del trabajo (0596).".into();
            conceptos.push(c);
        }
        if !ss.is_zero() {
            conceptos.push(concepto(
                TRABAJO,
                "Cotizaciones a la Seguridad Social a su cargo (gasto deducible)",
                "0013",
                ss,
                Money::ZERO,
                "nóminas",
            ));
        }
        // RETA (autónomo societario).
        if let Ok(r) = autonomo_societario::resumen(v, e.id, year, hoy) {
            let sociedad: Money = r
                .cuotas
                .iter()
                .filter(|q| q.pagador == Pagador::Sociedad)
                .map(|q| q.amount)
                .sum();
            let propias = r.cuotas_total - sociedad;
            if !sociedad.is_zero() {
                retribucion += sociedad;
                let mut c = concepto(
                    TRABAJO,
                    "Retribución en especie: cuota del RETA pagada por la sociedad",
                    "0004 / 0005",
                    sociedad,
                    r.ingreso_a_cuenta,
                    "Nóminas → Autónomo societario (cuotas pagadas)",
                );
                c.nota = "El ingreso a cuenta de la especie, si la sociedad no se lo repercute,                           se suma a la valoración (0005) y también va en retenciones (0596).                           TODO(VERIFY): la DGT también la ha calificado como renta dineraria."
                    .into();
                c.estimado = r.tipo_retencion.is_none();
                conceptos.push(c);
            }
            if !r.cuotas_total.is_zero() {
                let mut c = concepto(
                    TRABAJO,
                    "Cuotas del RETA del año (gasto deducible del trabajo)",
                    "0013",
                    r.cuotas_total,
                    Money::ZERO,
                    "Nóminas → Autónomo societario",
                );
                if !propias.is_zero() {
                    c.nota = format!("{propias} las pagó él mismo.");
                }
                conceptos.push(c);
            }
        }
    }

    // Dividendos (193).
    let mut div_bruto = Money::ZERO;
    let mut div_ret = Money::ZERO;
    for pago in socios::payments_of_year(v, year)? {
        for l in pago.lines.iter().filter(|l| norm(&l.nif) == nif) {
            div_bruto += l.gross;
            div_ret += l.withheld;
        }
    }
    if !div_bruto.is_zero() {
        let mut c = concepto(
            CAPITAL,
            "Dividendos y participaciones en beneficios",
            "0029",
            div_bruto,
            div_ret,
            "Cierre → Socios y dividendos (193)",
        );
        c.nota = "Base del ahorro. La retención, en la casilla de retenciones del capital                   mobiliario (0597)."
            .into();
        conceptos.push(c);
    }

    // Préstamos con la sociedad.
    let conn = v.connection();
    let pagados = prestamos::intereses_pagados(conn, start, end)?;
    let antes = start.previous_day().unwrap_or(start);
    let dias_año = (end - start).whole_days() + 1;
    let pn_ant = patrimonio_neto(v, year - 1)?;
    for d in prestamos::del_ejercicio(conn, start, end)? {
        if norm(&d.nif) != nif {
            continue;
        }
        let p = &d.prestamo;
        let cuadro = calc::cuadro(p);
        let inicio = calc::pendiente_a(p, &cuadro, antes).max(d.dispuesto);
        let medio = Money::from_cents((inicio.cents() + d.saldo_vivo.cents()) / 2);
        let desde = p.inicio.max(start);
        let dias = (end - desde).whole_days() + 1;
        let legal = interes(medio, prestamos::INTERES_LEGAL, dias, dias_año);
        match p.sentido {
            Sentido::Recibido => {
                // El socio presta a la sociedad: sus intereses son capital mobiliario.
                let (int, ret): (Money, Money) = pagados
                    .iter()
                    .filter(|x| x.prestamo == p.nombre && norm(&x.nif) == nif)
                    .fold((Money::ZERO, Money::ZERO), |(a, b), x| {
                        (a + x.intereses, b + x.retencion)
                    });
                let int = if int.is_zero() { d.intereses } else { int };
                if p.tipo.bp() == 0 {
                    let mut c = concepto(
                        CAPITAL,
                        format!(
                            "Intereses presuntos del préstamo «{}» a la sociedad (sin interés)",
                            p.nombre
                        ),
                        "0027",
                        legal,
                        Money::ZERO,
                        "Contabilidad → Préstamos",
                    );
                    c.nota = format!(
                        "El préstamo no tiene interés: se presume retribuido (art. 6.5 LIRPF) y,                          entre socio y sociedad, va por su valor de mercado (art. 41 LIRPF y                          art. 18 LIS). Estimación al interés legal del dinero ({}) sobre el                          capital medio ({medio}); el valor de mercado puede ser otro.",
                        prestamos::INTERES_LEGAL
                    );
                    c.estimado = true;
                    conceptos.push(c);
                } else {
                    let mut c = concepto(
                        CAPITAL,
                        format!("Intereses del préstamo «{}» a la sociedad", p.nombre),
                        "0027",
                        int,
                        ret,
                        "Contabilidad → Préstamos (123 y 193)",
                    );
                    c.nota = "Cesión a terceros de capitales propios (art. 25.2 LIRPF).".into();
                    conceptos.push(c);
                }
                if let Some(pn) = pn_ant
                    && persona.pct > 0
                {
                    let tope = Money::from_cents(por_participacion(pn, persona.pct).cents() * 3);
                    if d.saldo_vivo > tope.max(Money::ZERO) {
                        avisos.push(format!(
                            "El préstamo «{}» ({} pendiente) supera el triple de sus fondos                              propios en la sociedad ({tope}): los intereses de la parte que                              excede van a la base general, no a la del ahorro (art. 46.a LIRPF).",
                            p.nombre, d.saldo_vivo
                        ));
                    }
                }
            }
            Sentido::Concedido => {
                // La sociedad presta al socio.
                let diferencia = (legal - d.intereses).max(Money::ZERO);
                if p.tipo < prestamos::INTERES_LEGAL && !diferencia.is_zero() {
                    let trabaja = empleado.is_some();
                    let mut c = concepto(
                        if trabaja { TRABAJO } else { OTROS },
                        format!(
                            "Préstamo «{}» de la sociedad a un interés inferior al legal",
                            p.nombre
                        ),
                        if trabaja { "0004" } else { "—" },
                        diferencia,
                        Money::ZERO,
                        "Contabilidad → Préstamos",
                    );
                    c.nota = if trabaja {
                        format!(
                            "Retribución en especie: la diferencia entre el interés pagado ({})                              y el legal del dinero ({}) (art. 43.1.1.º.c LIRPF). Estimada con el                              capital medio; la sociedad debería ingresar a cuenta por ella.",
                            d.intereses,
                            prestamos::INTERES_LEGAL
                        )
                    } else {
                        "Operación vinculada a valor de mercado (art. 18 LIS): la diferencia                          puede tratarse como un dividendo encubierto o una liberalidad.                          TODO(VERIFY): calificación en el caso."
                            .into()
                    };
                    c.estimado = true;
                    conceptos.push(c);
                }
            }
        }
    }

    // Facturas del socio a la sociedad (alquiler o servicios).
    let (base, ret, n): (i64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(base), 0), COALESCE(SUM(withholding), 0), COUNT(*)
         FROM invoice_received
         WHERE status = 'revisada' AND UPPER(TRIM(supplier_nif)) = ?1
           AND issue_date BETWEEN ?2 AND ?3",
        taxpain_vault::rusqlite::params![nif, start.to_string(), end.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if n > 0 {
        let mut c = concepto(
            OTROS,
            "Facturas a la sociedad (alquiler o servicios)",
            "—",
            Money::from_cents(base),
            Money::from_cents(ret),
            &format!("{n} facturas recibidas revisadas"),
        );
        c.nota = "Un alquiler es rendimiento del capital inmobiliario (retención en el 115);                   unos servicios, de la actividad o del trabajo (art. 27.1 LIRPF, último                   párrafo, si son de la actividad de la sociedad)."
            .into();
        conceptos.push(c);
    }

    if conceptos.is_empty() {
        avisos.push(format!(
            "No hay pagos de la sociedad a {} en {year} (nóminas, cuotas, dividendos, préstamos              ni facturas).",
            persona.nombre
        ));
    }

    let patrimonio = patrimonio_socio(
        v,
        year,
        &persona,
        empleado.is_some(),
        retribucion,
        otros_rendimientos,
        hoy,
        &mut avisos,
    )?;

    Ok(InformeSocio {
        year,
        empresa: info.name,
        nif_empresa: info.nif.unwrap_or_default(),
        persona,
        conceptos,
        patrimonio,
        avisos,
        notas: vec![
            "Junta lo que la sociedad sabe y ha declarado (190, 193, 123 y 115); no calcula el              IRPF ni el Impuesto sobre el Patrimonio de la persona ni le dice qué hacer. Lo              demás (otras rentas, deducciones, mínimos) no está aquí."
                .into(),
            "Las casillas son las de Renta WEB de los últimos ejercicios y pueden cambiar:              cotéjalas con el borrador de la AEAT. Lo marcado como estimado no sale de un modelo              ni de un asiento."
                .into(),
        ],
    })
}

/// Patrimonio neto contable al cierre del ejercicio `year` (`None` si no hay contabilidad).
fn patrimonio_neto(v: &Vault, year: i32) -> Result<Option<Money>> {
    let asientos: Vec<_> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .collect();
    if asientos.is_empty() {
        return Ok(None);
    }
    let fy = accounting::fiscal_year(v, year)?;
    let s = patrimonial::saldos_a(&asientos, fy.end);
    Ok(Some(
        patrimonial::patrimonio_a(&s, fy.end, true, Money::ZERO).contable,
    ))
}

#[allow(clippy::too_many_arguments)]
fn patrimonio_socio(
    v: &Vault,
    year: i32,
    persona: &Persona,
    es_empleado: bool,
    retribucion: Money,
    otros: Option<Money>,
    hoy: Date,
    avisos: &mut Vec<String>,
) -> Result<PatrimonioSocio> {
    // A 31 de diciembre, el último balance aprobado suele ser el del ejercicio anterior (la
    // junta lo aprueba en los seis meses siguientes al cierre: art. 164 LSC).
    let ejercicio_balance = year - 1;
    let patrimonio_neto = patrimonio_neto(v, ejercicio_balance)?;
    let fecha_balance = match patrimonio_neto {
        Some(_) => Some(accounting::fiscal_year(v, ejercicio_balance)?.end),
        None => None,
    };
    let valor_teorico =
        patrimonio_neto.map(|pn| por_participacion(pn.max(Money::ZERO), persona.pct));
    let mut requisitos = Vec::new();

    // a) La entidad no gestiona un patrimonio (art. 4.Ocho.Dos.a LIP).
    let inf = patrimonial::informe(v, year, hoy).ok();
    let (estado, detalle) = match &inf {
        None => (
            Semaforo::SinDatos,
            "No hay contabilidad del año para el cálculo de entidad patrimonial.".to_owned(),
        ),
        Some(i) if i.provisional => (
            Semaforo::Dudoso,
            "Aún no ha terminado ningún trimestre del ejercicio: el cálculo es provisional."
                .to_owned(),
        ),
        Some(i) => {
            let sin_beneficios = i.ajustes.beneficios_actividad.is_none();
            let extra = if sin_beneficios {
                " Falta indicar los beneficios no distribuidos de actividad de los diez años                  anteriores (Contabilidad → Cierre → Entidad patrimonial): sin ellos el cálculo                  es el más desfavorable."
            } else {
                ""
            };
            let estado = match i.trimestres_lip {
                0 if sin_beneficios => Semaforo::Dudoso,
                0 => Semaforo::Cumple,
                1 => Semaforo::Dudoso,
                _ => Semaforo::NoCumple,
            };
            (
                estado,
                format!(
                    "{} de {} trimestres cerrados con más de la mitad del activo en valores o no                      afecto, sin lo comprado con beneficios de actividad (cuenta como más de 90                      días si son dos o más; uno, depende de las fechas). Media del art. 5.2 LIS:                      {}.{extra}",
                    i.trimestres_lip,
                    i.trimestres.iter().filter(|t| t.cerrado).count(),
                    i.media.pct
                ),
            )
        }
    };
    requisitos.push(Requisito {
        titulo: "La sociedad no gestiona un patrimonio mobiliario o inmobiliario".into(),
        estado,
        detalle,
        norma: "Art. 4.Ocho.Dos.a LIP".into(),
    });

    // b) Participación del 5 % (20 % con la familia).
    let (estado, detalle) = if persona.pct >= 50_000 {
        (
            Semaforo::Cumple,
            format!("Participación individual del {}.", pct_txt(persona.pct)),
        )
    } else if persona.pct > 0 {
        (
            Semaforo::Dudoso,
            format!(
                "Participación individual del {}: menos del 5 %. Vale si con el cónyuge,                  ascendientes, descendientes o colaterales de segundo grado llega al 20 %.",
                pct_txt(persona.pct)
            ),
        )
    } else {
        (
            Semaforo::NoCumple,
            "No figura como socio en el registro de socios.".to_owned(),
        )
    };
    requisitos.push(Requisito {
        titulo: "Participación de al menos el 5 % (20 % con su familia)".into(),
        estado,
        detalle,
        norma: "Art. 4.Ocho.Dos.b LIP".into(),
    });

    // c) Funciones de dirección con más del 50 % de sus rendimientos.
    let (estado, detalle) = if !persona.administrador && !es_empleado {
        (
            Semaforo::Dudoso,
            "No es administrador ni cobra de la sociedad: el requisito lo puede cumplir otra              persona de su grupo familiar (y entonces vale para todos)."
                .to_owned(),
        )
    } else if retribucion.is_zero() {
        (
            Semaforo::NoCumple,
            "No consta remuneración de la sociedad por sus funciones de dirección en el año              (salvo que la cobre otra persona de su grupo familiar)."
                .to_owned(),
        )
    } else if let Some(o) = otros {
        let total = retribucion + o.max(Money::ZERO);
        let r = ratio(retribucion, total);
        (
            if r > Rate::percent(50) {
                Semaforo::Cumple
            } else {
                Semaforo::NoCumple
            },
            format!(
                "Remuneración de la sociedad {retribucion} frente a {total} de rendimientos del                  trabajo y de actividades: {r} (tiene que ser más del 50 %)."
            ),
        )
    } else {
        (
            Semaforo::SinDatos,
            format!(
                "Remuneración de la sociedad {retribucion}. Indica sus otros rendimientos del                  trabajo y de actividades para comprobar que es más del 50 %."
            ),
        )
    };
    requisitos.push(Requisito {
        titulo: "Funciones de dirección con más del 50 % de sus rendimientos".into(),
        estado,
        detalle,
        norma: "Art. 4.Ocho.Dos.c LIP; RD 1704/1999".into(),
    });

    let exencion = if requisitos.iter().any(|r| r.estado == Semaforo::NoCumple) {
        Semaforo::NoCumple
    } else if requisitos.iter().all(|r| r.estado == Semaforo::Cumple) {
        Semaforo::Cumple
    } else {
        Semaforo::Dudoso
    };
    let proporcion_afecta = inf
        .as_ref()
        .filter(|i| i.media.activo.cents() > 0)
        .map(|i| {
            let afecto = i.media.activo - i.media.computa;
            ratio(afecto, i.media.activo)
        });
    let valor_exento_orientativo = match (exencion, valor_teorico, proporcion_afecta) {
        (Semaforo::NoCumple, _, _) => None,
        (_, Some(vt), Some(p)) => Some(vt.apply_rate(p)),
        _ => None,
    };
    if patrimonio_neto.is_none() {
        avisos.push(format!(
            "No hay contabilidad de {ejercicio_balance}: sin el balance no se puede calcular el              valor teórico de las participaciones."
        ));
    }
    Ok(PatrimonioSocio {
        ejercicio_balance,
        fecha_balance,
        patrimonio_neto,
        pct: persona.pct,
        valor_teorico,
        requisitos,
        exencion,
        proporcion_afecta,
        valor_exento_orientativo,
        notas: vec![
            format!(
                "Participaciones no cotizadas: valor teórico del último balance aprobado (art.                  16.Uno LIP). Se toma el de {ejercicio_balance}, que es el que suele estar                  aprobado el 31 de diciembre de {year}; si la junta no lo aprobó, es el anterior."
            ),
            "Si se cumplen los requisitos, la exención alcanza sólo la parte del valor que              corresponde a los activos necesarios para la actividad, menos sus deudas, frente al              patrimonio neto (art. 4.Ocho.Dos, último párrafo, LIP; arts. 5 y 6 RD 1704/1999).              La proporción de aquí es orientativa: sale del activo afecto medio del año."
                .into(),
            "Los mismos requisitos sirven para la reducción del 95 % en Sucesiones y Donaciones              (art. 20.2.c y 20.6 LISD), con sus condiciones propias."
                .into(),
        ],
    })
}

/// Párrafos del informe (PDF).
pub fn paragraphs(i: &InformeSocio) -> Vec<Para> {
    let mut p = vec![
        Para::Title(format!("Datos para la Renta y el Patrimonio {}", i.year)),
        Para::Text(format!(
            "{} · NIF {} · {}{}",
            i.persona.nombre,
            i.persona.nif,
            if i.persona.administrador {
                "administrador"
            } else {
                ""
            },
            if i.persona.socio {
                format!(
                    "{}socio con el {}",
                    if i.persona.administrador { " y " } else { "" },
                    pct_txt(i.persona.pct)
                )
            } else {
                String::new()
            }
        )),
        Para::Text(format!(
            "Datos de {} (NIF {}) del año {}.",
            i.empresa, i.nif_empresa, i.year
        )),
        Para::Gap,
        Para::Bold("1. Lo que la sociedad le ha pagado".into()),
    ];
    let mut apartado = "";
    for c in &i.conceptos {
        if c.apartado != apartado {
            apartado = &c.apartado;
            p.push(Para::Text(format!("{apartado}:")));
        }
        p.push(Para::Row(
            format!(
                "   {}{} [{}]",
                c.concepto,
                if c.estimado { " (estimado)" } else { "" },
                c.casilla
            ),
            c.importe.to_string(),
        ));
        if !c.retencion.is_zero() {
            p.push(Para::Row(
                "      Retención o ingreso a cuenta".into(),
                c.retencion.to_string(),
            ));
        }
        p.push(Para::Text(format!(
            "      Origen: {}.{}",
            c.fuente,
            if c.nota.is_empty() {
                String::new()
            } else {
                format!(" {}", c.nota)
            }
        )));
    }
    if i.conceptos.is_empty() {
        p.push(Para::Text("Nada en el año.".into()));
    }
    p.push(Para::Gap);
    let pt = &i.patrimonio;
    p.push(Para::Bold(
        "2. Impuesto sobre el Patrimonio: sus participaciones".into(),
    ));
    match (pt.patrimonio_neto, pt.fecha_balance) {
        (Some(pn), Some(f)) => {
            p.push(Para::Row(
                format!(
                    "Patrimonio neto del balance al {:02}/{:02}/{}",
                    f.day(),
                    u8::from(f.month()),
                    f.year()
                ),
                pn.to_string(),
            ));
        }
        _ => p.push(Para::Text(format!(
            "Sin contabilidad de {}: no hay balance.",
            pt.ejercicio_balance
        ))),
    }
    p.push(Para::Row("Participación".into(), pct_txt(pt.pct)));
    if let Some(vt) = pt.valor_teorico {
        p.push(Para::Row(
            "Valor teórico de las participaciones".into(),
            vt.to_string(),
        ));
    }
    p.push(Para::Text(format!(
        "Exención de empresa familiar (art. 4.Ocho.Dos LIP): {}.",
        pt.exencion.texto()
    )));
    for r in &pt.requisitos {
        p.push(Para::Text(format!(
            "   · {} ({}): {}. {}",
            r.titulo,
            r.norma,
            r.estado.texto(),
            r.detalle
        )));
    }
    if let (Some(pa), Some(ve)) = (pt.proporcion_afecta, pt.valor_exento_orientativo) {
        p.push(Para::Row(
            format!("Parte exenta orientativa (activo afecto medio {pa})"),
            ve.to_string(),
        ));
    }
    for n in &pt.notas {
        p.push(Para::Text(n.clone()));
    }
    if !i.avisos.is_empty() {
        p.push(Para::Gap);
        p.push(Para::Bold("Avisos".into()));
        for a in &i.avisos {
            p.push(Para::Text(format!("· {a}")));
        }
    }
    p.push(Para::Gap);
    for n in &i.notas {
        p.push(Para::Text(n.clone()));
    }
    p
}

/// El informe en PDF: (nombre del fichero, contenido).
pub fn pdf(
    v: &Vault,
    year: i32,
    nif: &str,
    otros_rendimientos: Option<Money>,
    hoy: Date,
) -> Result<(String, Vec<u8>)> {
    let i = informe(v, year, nif, otros_rendimientos, hoy)?;
    let bytes = pdf::text_document(
        &format!("Renta y Patrimonio {year} {}", i.persona.nif),
        &[paragraphs(&i)],
    )
    .map_err(AppError::Other)?;
    Ok((
        format!("Datos Renta y Patrimonio {year} {}.pdf", i.persona.nif),
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn participacion_e_intereses() {
        assert_eq!(
            por_participacion(Money::from_euros(100_000, 0), 600_000),
            Money::from_euros(60_000, 0)
        );
        assert_eq!(pct_txt(600_000), "60,00 %");
        // 10.000 € al 3,25 % medio año (182 de 365 días).
        assert_eq!(
            interes(Money::from_euros(10_000, 0), Rate::from_bp(325), 182, 365),
            Money::from_cents(16_205)
        );
        assert_eq!(
            ratio(Money::from_euros(30, 0), Money::from_euros(40, 0)),
            Rate::percent(75)
        );
    }
}
