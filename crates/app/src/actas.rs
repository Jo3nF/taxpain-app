//! Actas de la junta general, o decisiones del socio único (art. 15 LSC), con los datos de la
//! contabilidad y de los socios:
//! - aprobación de las cuentas y aplicación del resultado (art. 272 y 273 LSC), con la
//!   aplicación contabilizada en el cierre (`cierre::post_allocation`) o, si aún no lo está, la
//!   propuesta;
//! - reparto de dividendos (un pago de `socios`): bruto, retención, líquido y fecha de pago;
//! - dotación o traspaso de reservas.
//!
//! Y el certificado de retenciones de dividendos de cada socio (art. 108.3 RIRPF), con los
//! mismos pagos que el 193.
//!
//! Son borradores: lo que no sale de la contabilidad (lugar, hora, asistentes, firmas) va entre
//! «[…]». Se guardan en RTF (editable) o en PDF, como la memoria.

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::OptionalExtension;
use time::Date;

use crate::cierre;
use crate::error::{AppError, Result};
use crate::socios::{self, DividendPayment};

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn date_or_gap(d: Option<Date>) -> String {
    d.map_or_else(|| "[fecha]".into(), dmy)
}

/// Quién adopta los acuerdos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Organ {
    /// Un solo socio con el 100 %: decisiones del socio único (art. 15 LSC).
    SocioUnico { name: String, nif: String },
    /// Varios socios (o ninguno registrado): junta general.
    Junta,
}

pub fn organ(v: &Vault) -> Result<Organ> {
    let active: Vec<_> = socios::shareholders(v)?
        .into_iter()
        .filter(|s| s.active)
        .collect();
    Ok(match active.as_slice() {
        [s] if s.pct == 1_000_000 => Organ::SocioUnico {
            name: s.name.clone(),
            nif: s.nif.clone(),
        },
        _ => Organ::Junta,
    })
}

struct Company {
    name: String,
    nif: String,
}

fn company(v: &Vault) -> Result<Company> {
    let info = crate::companies::info(v)?;
    Ok(Company {
        name: info.name,
        nif: info.nif.unwrap_or_else(|| "[NIF]".into()),
    })
}

/// Encabezado del acta: quién, dónde y cuándo.
fn header(c: &Company, organ: &Organ, title: &str, date: Option<Date>) -> Vec<Para> {
    let when = date_or_gap(date);
    let mut p = vec![
        Para::Title(title.to_owned()),
        Para::Text(format!("{} · NIF {}", c.name, c.nif)),
        Para::Gap,
    ];
    match organ {
        Organ::SocioUnico { name, nif } => p.push(Para::Text(format!(
            "En [lugar], a {when}, {name} (NIF {nif}), socio único de {}, titular de la \
             totalidad de las participaciones sociales, ejerciendo las competencias de la junta \
             general (art. 15 de la Ley de Sociedades de Capital), adopta las siguientes \
             decisiones, que se consignan en el libro de actas:",
            c.name
        ))),
        Organ::Junta => p.push(Para::Text(format!(
            "En [lugar], a {when}, a las [hora], se reúne la junta general [ordinaria / \
             extraordinaria / universal] de {}, con la asistencia, presentes o representados, \
             de socios titulares del [porcentaje] % del capital social, [según la lista de \
             asistentes que se adjunta / encontrándose presente la totalidad del capital, que \
             acepta por unanimidad la celebración de la junta y el orden del día (art. 178 \
             LSC)]. Actúa como presidente [nombre] y como secretario [nombre]. Tras la \
             deliberación, se adoptan [por unanimidad / con el voto favorable del [porcentaje] \
             % del capital] los siguientes acuerdos:",
            c.name
        ))),
    }
    p.push(Para::Gap);
    p
}

fn footer(organ: &Organ) -> Vec<Para> {
    let mut p = vec![Para::Gap];
    match organ {
        Organ::SocioUnico { name, .. } => {
            p.push(Para::Text(
                "Y para que conste, firma el socio único el acta de sus decisiones.".into(),
            ));
            p.push(Para::Gap);
            p.push(Para::Text(format!("Fdo.: {name}")));
        }
        Organ::Junta => {
            p.push(Para::Text(
                "Sin más asuntos que tratar, se levanta la sesión, y se redacta el acta, que es \
                 leída y aprobada [al final de la reunión / dentro de los quince días siguientes \
                 por el presidente y dos socios interventores] (art. 202 LSC)."
                    .into(),
            ));
            p.push(Para::Gap);
            p.push(Para::Text(
                "Vº Bº El presidente: [nombre]          El secretario: [nombre]".into(),
            ));
        }
    }
    p
}

fn verb(organ: &Organ) -> &'static str {
    match organ {
        Organ::SocioUnico { .. } => "Decisión",
        Organ::Junta => "Acuerdo",
    }
}

/// Partida de la aplicación del resultado: texto y cuenta.
fn allocation_label(account: &str, name: Option<&str>) -> String {
    let p = |prefix: &str| account.starts_with(prefix);
    if p(cierre::CAPITALIZATION_ACCOUNT) {
        "A reserva de capitalización (art. 25 LIS)".into()
    } else if name.is_some_and(|n| n.to_lowercase().contains("nivelaci")) {
        "A reserva de nivelación de bases imponibles (art. 105 LIS)".into()
    } else if p("112") {
        "A reserva legal".into()
    } else if p("113") {
        "A reservas voluntarias".into()
    } else if p("114") {
        format!("A {}", name.unwrap_or("reservas especiales"))
    } else if p("121") {
        "A compensación de pérdidas de ejercicios anteriores".into()
    } else if p("526") || p("557") {
        "A dividendos".into()
    } else if p("120") {
        "A remanente".into()
    } else {
        format!("A {}", name.unwrap_or(account))
    }
}

type AllocationRows = (
    cierre::AllocationProposal,
    Vec<(String, Money)>,
    Option<Date>,
);

/// Aplicación del resultado de `year` en partidas (texto, importe): la contabilizada o la
/// propuesta, y la fecha de la junta si está contabilizada.
fn allocation_rows(v: &Vault, year: i32) -> Result<AllocationRows> {
    let a = cierre::allocation(v, year)?;
    let conn = v.connection();
    if let Some(id) = a.entry_id {
        let date: String =
            conn.query_row("SELECT date FROM journal_entry WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        let date = Date::parse(
            &date,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .map_err(|e| AppError::Other(e.to_string()))?;
        let mut st = conn.prepare(
            "SELECT l.account, SUM(l.credit - l.debit), a.name FROM journal_line l
               LEFT JOIN account a ON a.code = l.account
              WHERE l.entry_id = ?1 AND l.account NOT LIKE '129%'
              GROUP BY l.account ORDER BY MIN(l.id)",
        )?;
        let rows = st.query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out: Vec<(String, Money)> = Vec::new();
        for row in rows {
            let (acc, cents, name) = row?;
            let label = allocation_label(&acc, name.as_deref());
            let m = Money::from_cents(cents.abs());
            // 557 y 526 juntas: un solo dividendo.
            if let Some(x) = out.iter_mut().find(|(l, _)| *l == label) {
                x.1 += m;
            } else {
                out.push((label, m));
            }
        }
        return Ok((a, out, Some(date)));
    }
    let s = a.suggested;
    let out = [
        (allocation_label("112", None), s.legal),
        (allocation_label("121", None), s.losses),
        (
            allocation_label(cierre::CAPITALIZATION_ACCOUNT, None),
            s.capitalization,
        ),
        (allocation_label("113", None), s.voluntary),
        (allocation_label("526", None), s.dividends),
        (allocation_label("120", None), s.remanente),
    ]
    .into_iter()
    .filter(|(_, m)| m.cents() > 0)
    .collect();
    Ok((a, out, None))
}

/// Acta de aprobación de las cuentas anuales de `year` y de la aplicación del resultado.
pub fn cuentas(v: &Vault, year: i32) -> Result<Vec<Para>> {
    let c = company(v)?;
    let organ = organ(v)?;
    let (a, rows, date) = allocation_rows(v, year)?;
    let fy = crate::accounting::fiscal_year(v, year)?;
    let verb = verb(&organ);
    let mut p = header(
        &c,
        &organ,
        &format!("Acta de aprobación de las cuentas anuales {year}"),
        date,
    );
    p.push(Para::Bold(format!(
        "{verb} primero. Aprobación de las cuentas anuales"
    )));
    p.push(Para::Text(format!(
        "Aprobar las cuentas anuales del ejercicio cerrado el {} (balance, cuenta de pérdidas y \
         ganancias [y estado de cambios en el patrimonio neto] y memoria, en el modelo [abreviado \
         / de PYMES]), formuladas por el órgano de administración el [fecha de formulación], que \
         arrojan un resultado de {} {} ({}).",
        dmy(fy.end),
        if a.result.cents() < 0 {
            "pérdidas de"
        } else {
            "beneficios de"
        },
        if a.result.cents() < 0 {
            -a.result
        } else {
            a.result
        },
        "[sin auditar / con el informe de auditoría de [auditor]]"
    )));
    p.push(Para::Bold(format!(
        "{verb} segundo. Aplicación del resultado"
    )));
    if a.result.cents() < 0 {
        p.push(Para::Text(
            "Aplicar las pérdidas del ejercicio a resultados negativos de ejercicios anteriores \
             (cuenta 121), para compensarlas con beneficios futuros."
                .into(),
        ));
        p.push(Para::Row(
            "Base de reparto: pérdidas del ejercicio".into(),
            (-a.result).to_string(),
        ));
        p.push(Para::Row(
            "A resultados negativos de ejercicios anteriores".into(),
            (-a.result).to_string(),
        ));
    } else {
        p.push(Para::Text(
            "Aplicar el resultado del ejercicio según la siguiente propuesta:".into(),
        ));
        p.push(Para::Row(
            "Base de reparto: pérdidas y ganancias".into(),
            a.result.to_string(),
        ));
        for (l, m) in &rows {
            p.push(Para::Row(l.clone(), m.to_string()));
        }
        let dividend = rows
            .iter()
            .find(|(l, _)| l == "A dividendos")
            .map(|(_, m)| *m);
        if let Some(d) = dividend {
            let interim = a.interim_dividend.min(d);
            if interim.cents() > 0 {
                p.push(Para::Text(format!(
                    "Del dividendo, {interim} se repartió a cuenta durante el ejercicio y se \
                     ratifica; el resto, {}, se pagará el [fecha de pago], con la retención que \
                     corresponda a cada socio.",
                    d - interim
                )));
            } else {
                p.push(Para::Text(
                    "El dividendo se repartirá entre los socios en proporción a su participación \
                     en el capital y se pagará el [fecha de pago], con la retención que \
                     corresponda a cada socio (art. 94.1 RIRPF: si no se fija fecha, es exigible \
                     el día siguiente al acuerdo)."
                        .into(),
                ));
            }
        }
    }
    if date.is_none() {
        p.push(Para::Text(
            "[Propuesta: la aplicación aún no está contabilizada en TaxPain; cuando la junta la \
             apruebe, contabilízala en el cierre y vuelve a generar el acta.]"
                .into(),
        ));
    }
    p.push(Para::Bold(format!("{verb} tercero. Gestión social")));
    p.push(Para::Text(
        "Aprobar la gestión del órgano de administración durante el ejercicio.".into(),
    ));
    p.push(Para::Bold(format!("{verb} cuarto. Facultades")));
    p.push(Para::Text(
        "Facultar a [administrador] para que deposite las cuentas en el Registro Mercantil (art. \
         279 LSC) y expida las certificaciones de estos acuerdos."
            .into(),
    ));
    p.extend(footer(&organ));
    Ok(p)
}

fn payment(v: &Vault, id: i64) -> Result<DividendPayment> {
    let date: Option<String> = v
        .connection()
        .query_row(
            "SELECT date FROM dividend_payment WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let date = date.ok_or(AppError::NotFound)?;
    let d = Date::parse(
        &date,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map_err(|e| AppError::Other(e.to_string()))?;
    socios::payments_between(v, d, d)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or(AppError::NotFound)
}

/// Acta del acuerdo de reparto de dividendos de un pago registrado en Socios.
pub fn dividendo(v: &Vault, payment_id: i64) -> Result<Vec<Para>> {
    let pay = payment(v, payment_id)?;
    dividend_paragraphs(&company(v)?, &organ(v)?, &pay)
}

fn dividend_paragraphs(c: &Company, organ: &Organ, pay: &DividendPayment) -> Result<Vec<Para>> {
    let verb = verb(organ);
    let mut p = header(c, organ, "Acta de reparto de dividendos", None);
    p.push(Para::Bold(format!("{verb} único. Reparto de dividendos")));
    p.push(Para::Text(format!(
        "Repartir un dividendo bruto de {} con cargo a [resultado del ejercicio [año] / reservas \
         voluntarias de libre disposición], después de comprobar que el patrimonio neto no queda \
         por debajo del capital social (art. 273.2 LSC). Se paga el {} a los socios en \
         proporción a su participación, con la retención a cuenta que corresponde a cada uno:",
        pay.gross,
        dmy(pay.date)
    )));
    for l in &pay.lines {
        p.push(Para::Row(
            format!("{} (NIF {}) · bruto · retención {}", l.name, l.nif, l.rate),
            format!("{} · {}", l.gross, l.withheld),
        ));
    }
    p.push(Para::Row("Total bruto".into(), pay.gross.to_string()));
    p.push(Para::Row("Retención".into(), pay.withheld.to_string()));
    p.push(Para::Row("Líquido a pagar".into(), pay.net.to_string()));
    p.extend(footer(organ));
    Ok(p)
}

/// Dotación o traspaso de reservas (de una cuenta de fondos propios a otra).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReserveChange {
    pub date: Option<Date>,
    /// Cuenta de origen (p. ej. 113 o 129) y de destino (112, 113, 1149…).
    pub from: String,
    pub to: String,
    pub amount: Money,
    /// Motivo (p. ej. «completar la reserva legal»).
    #[serde(default)]
    pub reason: String,
}

fn reserve_name(code: &str) -> &'static str {
    match code {
        c if c.starts_with(cierre::CAPITALIZATION_ACCOUNT) => {
            "reserva de capitalización (art. 25 LIS)"
        }
        c if c.starts_with("112") => "reserva legal",
        c if c.starts_with("113") => "reservas voluntarias",
        c if c.starts_with("114") => "reservas especiales",
        c if c.starts_with("120") => "remanente",
        c if c.starts_with("121") => "resultados negativos de ejercicios anteriores",
        c if c.starts_with("129") => "resultado del ejercicio",
        c if c.starts_with("526") => "dividendo a pagar",
        _ => "[cuenta]",
    }
}

/// Acta de dotación o traspaso de reservas.
pub fn reservas(v: &Vault, r: &ReserveChange) -> Result<Vec<Para>> {
    if r.amount.cents() <= 0 {
        return Err(AppError::Other("el importe ha de ser positivo".into()));
    }
    let organ = organ(v)?;
    let verb = verb(&organ);
    let mut p = header(&company(v)?, &organ, "Acta de dotación de reservas", r.date);
    p.push(Para::Bold(format!("{verb} único. Reservas")));
    let reason = r.reason.trim();
    p.push(Para::Text(format!(
        "Traspasar {} de {} (cuenta {}) a {} (cuenta {}){}.",
        r.amount,
        reserve_name(&r.from),
        r.from,
        reserve_name(&r.to),
        r.to,
        if reason.is_empty() {
            ", [motivo]".to_owned()
        } else {
            format!(", para {reason}")
        }
    )));
    if r.to.starts_with(cierre::CAPITALIZATION_ACCOUNT) {
        p.push(Para::Text(
            "La reserva es indisponible durante cinco años (art. 25.1.b LIS) y figura en el \
             balance con absoluta separación y título apropiado."
                .into(),
        ));
    }
    if r.from.starts_with("112") {
        p.push(Para::Text(
            "[Ojo: la reserva legal sólo puede destinarse a compensar pérdidas o, en lo que \
             exceda del 10 % del capital ya aumentado, a ampliar capital (art. 274.2 LSC).]"
                .into(),
        ));
    }
    p.extend(footer(&organ));
    Ok(p)
}

/// Actas con fecha en el ejercicio `year`, por orden: aprobación de las cuentas del anterior
/// (si su aplicación está contabilizada en `year`) y repartos de dividendos. Es el contenido del
/// libro de actas del ejercicio.
pub fn of_year(v: &Vault, year: i32) -> Result<Vec<(Date, String, Vec<Para>)>> {
    let fy = crate::accounting::fiscal_year(v, year)?;
    let mut out = Vec::new();
    if let Ok((_, _, Some(d))) = allocation_rows(v, year - 1)
        && fy.contains(d)
    {
        out.push((
            d,
            format!("Aprobación de las cuentas {}", year - 1),
            cuentas(v, year - 1)?,
        ));
    }
    let c = company(v)?;
    let organ = organ(v)?;
    for pay in socios::payments_between(v, fy.start, fy.end)? {
        let mut p = dividend_paragraphs(&c, &organ, &pay)?;
        // En el libro, la fecha del acuerdo; sin ella, la del pago entre corchetes.
        if let Some(Para::Text(t)) = p.get_mut(3) {
            *t = t.replacen(
                "[fecha]",
                &format!("[fecha del acuerdo; pago el {}]", dmy(pay.date)),
                1,
            );
        }
        out.push((
            pay.date,
            format!("Reparto de dividendos {}", dmy(pay.date)),
            p,
        ));
    }
    out.sort_by_key(|(d, ..)| *d);
    Ok(out)
}

/// PDF de una o varias actas (cada una en página nueva).
pub fn pdf(title: &str, docs: &[Vec<Para>]) -> Result<Vec<u8>> {
    pdf::text_document(title, docs).map_err(AppError::Other)
}

/// RTF de una o varias actas (cada una en página nueva).
pub fn rtf(docs: &[Vec<Para>]) -> String {
    crate::memoria::to_rtf_docs(docs)
}

// ---------- Certificado de retenciones ----------

/// Un pago en el certificado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CertificateLine {
    pub date: Date,
    pub gross: Money,
    pub rate: taxpain_core::money::Rate,
    pub withheld: Money,
    pub net: Money,
}

/// Certificado de retenciones de un socio en el año natural (art. 108.3 RIRPF).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Certificate {
    pub year: i32,
    pub nif: String,
    pub name: String,
    pub resident: bool,
    pub lines: Vec<CertificateLine>,
    pub gross: Money,
    pub withheld: Money,
    pub net: Money,
}

/// Certificados del año natural `year`, uno por socio (por NIF), con los pagos del 193.
pub fn certificates(v: &Vault, year: i32) -> Result<Vec<Certificate>> {
    let mut out: Vec<Certificate> = Vec::new();
    for pay in socios::payments_of_year(v, year)? {
        for l in pay.lines {
            let i = if let Some(i) = out.iter().position(|c| c.nif == l.nif) {
                i
            } else {
                out.push(Certificate {
                    year,
                    nif: l.nif.clone(),
                    name: l.name.clone(),
                    resident: l.resident,
                    lines: Vec::new(),
                    gross: Money::ZERO,
                    withheld: Money::ZERO,
                    net: Money::ZERO,
                });
                out.len() - 1
            };
            let c = &mut out[i];
            c.gross += l.gross;
            c.withheld += l.withheld;
            c.net += l.net;
            c.lines.push(CertificateLine {
                date: pay.date,
                gross: l.gross,
                rate: l.rate,
                withheld: l.withheld,
                net: l.net,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn certificate_paragraphs(c: &Company, cert: &Certificate) -> Vec<Para> {
    let mut p = vec![
        Para::Title(format!(
            "Certificado de retenciones e ingresos a cuenta {}",
            cert.year
        )),
        Para::Text("Rendimientos del capital mobiliario: dividendos (art. 25.1.a LIRPF)".into()),
        Para::Gap,
        Para::Bold("Pagador".into()),
        Para::Text(format!("{} · NIF {}", c.name, c.nif)),
        Para::Bold("Perceptor".into()),
        Para::Text(format!("{} · NIF {}", cert.name, cert.nif)),
        Para::Gap,
        Para::Text(format!(
            "{} certifica que en el año {} ha satisfecho al perceptor los siguientes dividendos, \
             con las retenciones que se indican, ingresadas en el modelo 123 y declaradas en el \
             resumen anual 193:",
            c.name, cert.year
        )),
    ];
    for l in &cert.lines {
        p.push(Para::Row(
            format!("{} · bruto {} · retención {}", dmy(l.date), l.gross, l.rate),
            format!("{} · líquido {}", l.withheld, l.net),
        ));
    }
    p.push(Para::Gap);
    p.push(Para::Row("Importe íntegro".into(), cert.gross.to_string()));
    p.push(Para::Row("Retenciones".into(), cert.withheld.to_string()));
    p.push(Para::Row("Importe líquido".into(), cert.net.to_string()));
    if !cert.resident {
        p.push(Para::Text(
            "[Socio no residente: la retención se declara en el 216 y el 296 (IRNR), no en el \
             123 y el 193.]"
                .into(),
        ));
    }
    p.push(Para::Gap);
    p.push(Para::Text(
        "Y para que conste a efectos de su declaración del impuesto (art. 108.3 RIRPF), se \
         expide el presente certificado en [lugar], a [fecha]."
            .into(),
    ));
    p.push(Para::Gap);
    p.push(Para::Text("Fdo.: [administrador], [cargo]".into()));
    p
}

/// Párrafos de los certificados de `year` (todos, o el del socio con `nif`).
pub fn certificate_docs(v: &Vault, year: i32, nif: Option<&str>) -> Result<Vec<Vec<Para>>> {
    let c = company(v)?;
    let docs: Vec<_> = certificates(v, year)?
        .iter()
        .filter(|x| nif.is_none_or(|n| n == x.nif))
        .map(|x| certificate_paragraphs(&c, x))
        .collect();
    if docs.is_empty() {
        return Err(AppError::Other(format!(
            "no hay dividendos pagados en {year}{}",
            nif.map(|n| format!(" a {n}")).unwrap_or_default()
        )));
    }
    Ok(docs)
}

/// Qué acta generar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Cuentas { year: i32 },
    Dividendo { payment_id: i64 },
    Reservas(ReserveChange),
    Certificados { year: i32, nif: Option<String> },
}

/// Documentos de `r` (un acta, o un certificado por socio).
pub fn docs(v: &Vault, r: &Request) -> Result<Vec<Vec<Para>>> {
    Ok(match r {
        Request::Cuentas { year } => vec![cuentas(v, *year)?],
        Request::Dividendo { payment_id } => vec![dividendo(v, *payment_id)?],
        Request::Reservas(x) => vec![reservas(v, x)?],
        Request::Certificados { year, nif } => certificate_docs(v, *year, nif.as_deref())?,
    })
}
