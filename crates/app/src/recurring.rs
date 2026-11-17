//! Facturas periódicas (cuotas mensuales, mantenimiento, hosting…) y «Duplicar».
//!
//! Una plantilla sale de una factura ya emitida: mismo cliente, líneas y descripción, cada
//! `every_months` meses. Nunca se emite sola: el panel avisa de las que tocan y el usuario las
//! emite (cada emisión crea un registro VERI*FACTU permanente y consume número de la serie).
//! En los textos, `{mes}`, `{trimestre}` y `{año}` se cambian por los del periodo.
//!
//! Se guardan en los ajustes (`invoice_templates`): son pocas y no hace falta tabla propia.

use serde::{Deserialize, Serialize};
use taxpain_core::fx::ForeignCurrency;
use taxpain_core::invoice::{Invoice, InvoiceKind};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::{self, InvoiceDraft, IssuedInvoice, now_str};
use crate::settings::Language;

const KEY: &str = "invoice_templates";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceTemplate {
    pub id: String,
    pub name: String,
    /// Cliente, líneas y textos (la fecha se pone al emitir).
    pub draft: InvoiceDraft,
    #[serde(default)]
    pub language: Option<Language>,
    pub every_months: u32,
    /// Primera fecha y cuántas se han emitido: la siguiente es `start + n × every_months`
    /// el día `day` (sin arrastrar el día: 31/1, 28/2, 31/3…).
    pub start: Date,
    pub issued: u32,
    /// Día del mes de las fechas (31: el último de los meses más cortos). Las plantillas
    /// anteriores no lo tienen: el de `start`.
    #[serde(default)]
    pub day: Option<u8>,
    pub end: Option<Date>,
    pub active: bool,
    /// Última emitida: número y fecha.
    pub last: Option<(String, Date)>,
}

impl InvoiceTemplate {
    /// Fecha del periodo `n` (0 = `start`), sin mirar el final.
    fn nth(&self, n: u32) -> Date {
        let d = months_after(self.start, n.saturating_mul(self.every_months));
        match self.day {
            Some(day) => with_day(d, day),
            None => d,
        }
    }

    /// Fecha de la siguiente (o `None` si ya terminó).
    pub fn next_date(&self) -> Option<Date> {
        let d = self.nth(self.issued);
        match self.end {
            Some(e) if d > e => None,
            _ => Some(d),
        }
    }

    /// Las plantillas creadas antes de guardar el día partían de la fecha ya ajustada al mes
    /// (31/1 → 28/2) y se quedaban en el 28: si aún no se ha emitido ninguna y la siguiente es
    /// la de la factura de origen más la frecuencia, se toma el día de esa factura.
    fn repair_day(&mut self) {
        if self.day.is_some() || self.issued != 0 {
            return;
        }
        if let Some((_, origin)) = &self.last
            && origin.day() > self.start.day()
            && months_after(*origin, self.every_months) == self.start
        {
            self.day = Some(origin.day());
        }
    }
}

/// La misma fecha con el día `day` (o el último del mes si no lo tiene).
fn with_day(d: Date, day: u8) -> Date {
    let day = day.clamp(1, d.month().length(d.year()));
    Date::from_calendar_date(d.year(), d.month(), day).unwrap_or(d)
}

/// Plantilla con su siguiente fecha (para la interfaz).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateRow {
    #[serde(flatten)]
    pub template: InvoiceTemplate,
    pub next_date: Option<Date>,
    /// Le toca (siguiente fecha hoy o antes).
    pub due: bool,
    pub client: Option<String>,
}

/// Cambios desde la interfaz.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TemplatePatch {
    pub name: String,
    pub every_months: u32,
    /// Nueva siguiente fecha, sólo si el usuario la cambia: la serie vuelve a empezar en ella
    /// (con su día). Si no, sigue donde iba (una plantilla terminada no vuelve a facturar lo
    /// ya emitido al cambiarle el nombre).
    #[serde(default)]
    pub next_date: Option<Date>,
    pub end: Option<Date>,
    pub active: bool,
    /// Concepto de cada línea (con `{mes}`…) y precio en céntimos, si se cambian.
    #[serde(default)]
    pub line_descriptions: Option<Vec<String>>,
    #[serde(default)]
    pub line_prices: Option<Vec<i64>>,
}

/// `d` más `months` meses, con el día ajustado al último del mes si no existe.
pub fn months_after(d: Date, months: u32) -> Date {
    let total = i64::from(u8::from(d.month())) - 1 + i64::from(months);
    let year = d.year() + i32::try_from(total.div_euclid(12)).unwrap_or(0);
    let month = Month::try_from(u8::try_from(total.rem_euclid(12) + 1).unwrap_or(1))
        .unwrap_or(Month::January);
    Date::from_calendar_date(year, month, d.day().min(month.length(year))).unwrap_or(d)
}

fn load(v: &Vault) -> Result<Vec<InvoiceTemplate>> {
    let mut list: Vec<InvoiceTemplate> = v.setting(KEY)?.unwrap_or_default();
    for t in &mut list {
        t.repair_day();
    }
    Ok(list)
}

fn store(v: &Vault, list: &[InvoiceTemplate]) -> Result<()> {
    v.set_setting(KEY, &list)?;
    Ok(())
}

const MONTHS_ES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];
const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Cambia `{mes}`, `{trimestre}` y `{año}` (y `{month}`, `{quarter}`, `{year}`) por los de `d`.
pub fn fill(text: &str, d: Date, lang: Language) -> String {
    let m = usize::from(u8::from(d.month())) - 1;
    let q = m / 3 + 1;
    let month = match lang {
        Language::Es => format!("{} {}", MONTHS_ES[m], d.year()),
        Language::En => format!("{} {}", MONTHS_EN[m], d.year()),
    };
    let quarter = match lang {
        Language::Es => format!("{q}T {}", d.year()),
        Language::En => format!("Q{q} {}", d.year()),
    };
    text.replace("{mes}", &month)
        .replace("{month}", &month)
        .replace("{trimestre}", &quarter)
        .replace("{quarter}", &quarter)
        .replace("{año}", &d.year().to_string())
        .replace("{year}", &d.year().to_string())
}

/// Borrador de una factura emitida (para «Duplicar» y para crear una plantilla), sin fecha
/// propia ni rectificación.
pub fn draft_of(v: &Vault, invoice_id: i64) -> Result<InvoiceDraft> {
    let data: String = v.connection().query_row(
        "SELECT data FROM invoice_issued WHERE id = ?1",
        [invoice_id],
        |r| r.get(0),
    )?;
    #[derive(Deserialize)]
    struct Stored {
        invoice: Invoice,
        #[serde(default)]
        fx: Option<invoicing::StoredFx>,
        #[serde(default)]
        totals: Option<taxpain_core::invoice::Totals>,
    }
    let stored =
        serde_json::from_str::<Stored>(&data).map_err(|e| AppError::Other(e.to_string()))?;
    let mut inv = stored.invoice;
    // Ventanilla única: la nueva lleva el mismo tipo y clase de operación.
    let oss = stored
        .totals
        .and_then(|t| t.oss)
        .map(|o| crate::oss::OssDraft {
            rate: o.rate,
            supply: o.supply,
        });
    // En moneda extranjera, las líneas de la factura (en la moneda) y su moneda, con el tipo
    // de entonces: al emitir la nueva hay que poner el del día (se avisa si no).
    let currency = stored.fx.map(|f| {
        inv.lines = f.lines;
        f.currency
    });
    if matches!(inv.kind, InvoiceKind::Rectificativa(_)) {
        return Err(AppError::Other(
            "una rectificativa no se duplica ni se repite".into(),
        ));
    }
    Ok(InvoiceDraft {
        kind: if inv.recipient.is_some() {
            InvoiceKind::Completa
        } else {
            InvoiceKind::Simplificada
        },
        series: None,
        issue_date: inv.issue_date,
        operation_date: None,
        recipient: inv.recipient,
        lines: inv.lines,
        rectified: vec![],
        rectified_amounts: None,
        description: inv.description,
        notes: inv.notes,
        currency,
        oss,
    })
}

fn new_id() -> Result<String> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(hex::encode(b))
}

/// Crea una plantilla a partir de una factura emitida: la siguiente, `every_months` meses
/// después de ella.
pub fn create_from_invoice(
    v: &Vault,
    invoice_id: i64,
    every_months: u32,
    now: OffsetDateTime,
) -> Result<InvoiceTemplate> {
    if !(1..=12).contains(&every_months) {
        return Err(AppError::Other("la frecuencia va de 1 a 12 meses".into()));
    }
    let draft = draft_of(v, invoice_id)?;
    let Some(client) = draft.recipient.as_ref() else {
        return Err(AppError::Other(
            "sólo se repiten facturas con cliente identificado".into(),
        ));
    };
    let number: String = v.connection().query_row(
        "SELECT number FROM invoice_issued WHERE id = ?1",
        [invoice_id],
        |r| r.get(0),
    )?;
    // El idioma de la factura de origen (una cuota en inglés sigue en inglés).
    let language = invoicing::language_of(v, invoice_id)?;
    let t = InvoiceTemplate {
        id: new_id()?,
        name: format!("{} · {}", client.name, draft.description),
        language,
        every_months,
        start: months_after(draft.issue_date, every_months),
        issued: 0,
        // El día de la factura de origen: una del 31/1 sigue el 28/2, el 31/3…
        day: Some(draft.issue_date.day()),
        end: None,
        active: true,
        last: Some((number, draft.issue_date)),
        draft,
    };
    let mut list = load(v)?;
    list.push(t.clone());
    store(v, &list)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "invoice_template.create",
        &t.id,
        &serde_json::json!({ "every_months": every_months }),
    )?;
    Ok(t)
}

/// Plantillas con su siguiente fecha.
pub fn list(v: &Vault, today: Date) -> Result<Vec<TemplateRow>> {
    let mut rows: Vec<TemplateRow> = load(v)?
        .into_iter()
        .map(|t| {
            let next = t.next_date();
            TemplateRow {
                due: t.active && next.is_some_and(|d| d <= today),
                next_date: next,
                client: t.draft.recipient.as_ref().map(|r| r.name.clone()),
                template: t,
            }
        })
        .collect();
    rows.sort_by_key(|r| (r.next_date.is_none(), r.next_date));
    Ok(rows)
}

/// Cuántas tocan hoy (para el panel).
pub fn due_count(v: &Vault, today: Date) -> Result<usize> {
    Ok(list(v, today)?.iter().filter(|r| r.due).count())
}

pub fn update(v: &Vault, id: &str, p: &TemplatePatch, now: OffsetDateTime) -> Result<()> {
    if !(1..=12).contains(&p.every_months) {
        return Err(AppError::Other("la frecuencia va de 1 a 12 meses".into()));
    }
    if p.name.trim().is_empty() {
        return Err(AppError::Other("pon un nombre".into()));
    }
    let mut list = load(v)?;
    let t = list
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(AppError::NotFound)?;
    t.name = p.name.trim().to_owned();
    match p.next_date {
        // La siguiente pasa a ser la indicada, y su día el de las demás.
        Some(next) if Some(next) != t.next_date() => {
            t.start = next;
            t.issued = 0;
            t.day = Some(next.day());
        }
        // Otra frecuencia: la siguiente sigue siendo la que tocaba y desde ella, la nueva.
        _ if p.every_months != t.every_months => {
            t.start = t.nth(t.issued);
            t.issued = 0;
        }
        _ => {}
    }
    t.every_months = p.every_months;
    t.end = p.end;
    t.active = p.active;
    if let Some(ds) = &p.line_descriptions {
        if ds.len() != t.draft.lines.len() || ds.iter().any(|d| d.trim().is_empty()) {
            return Err(AppError::Other("pon el concepto de cada línea".into()));
        }
        for (l, d) in t.draft.lines.iter_mut().zip(ds) {
            l.description = d.trim().to_owned();
        }
        if let Some(first) = ds.first() {
            t.draft.description = first.trim().to_owned();
        }
    }
    if let Some(ps) = &p.line_prices {
        if ps.len() != t.draft.lines.len() {
            return Err(AppError::Other("falta el precio de alguna línea".into()));
        }
        for (l, p) in t.draft.lines.iter_mut().zip(ps) {
            l.unit_price = taxpain_core::money::Money::from_cents(*p);
        }
    }
    store(v, &list)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "invoice_template.update",
        id,
        &serde_json::json!({ "every_months": p.every_months, "active": p.active }),
    )?;
    Ok(())
}

pub fn delete(v: &Vault, id: &str, now: OffsetDateTime) -> Result<()> {
    let mut list = load(v)?;
    let before = list.len();
    list.retain(|t| t.id != id);
    if list.len() == before {
        return Err(AppError::NotFound);
    }
    store(v, &list)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "invoice_template.delete",
        id,
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// La factura que saldría de la plantilla `id` hoy (fecha: la que toca o, si ya pasó, hoy con
/// la del periodo como fecha de la operación).
pub fn preview_draft(v: &Vault, id: &str, today: Date) -> Result<(InvoiceDraft, Option<Language>)> {
    let (d, language, _) = preview_period(v, id, today)?;
    Ok((d, language))
}

/// [`preview_draft`] con la fecha del periodo que se factura.
fn preview_period(
    v: &Vault,
    id: &str,
    today: Date,
) -> Result<(InvoiceDraft, Option<Language>, Date)> {
    let t = load(v)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or(AppError::NotFound)?;
    let period = t
        .next_date()
        .ok_or_else(|| AppError::Other("la plantilla ya terminó".into()))?;
    let lang = t.language.unwrap_or(
        invoicing::settings(v)?
            .map(|s| s.language)
            .unwrap_or_default(),
    );
    let mut d = t.draft.clone();
    if today > period {
        // Se emite hoy (la numeración es correlativa por fecha); el servicio es del periodo.
        d.issue_date = today;
        d.operation_date = Some(period);
    } else {
        d.issue_date = period;
        d.operation_date = None;
    }
    d.description = fill(&d.description, period, lang);
    for l in &mut d.lines {
        l.description = fill(&l.description, period, lang);
    }
    if let Some(n) = d.notes.as_mut() {
        *n = fill(n, period, lang);
    }
    Ok((d, t.language, period))
}

/// Qué hacer con una factura atrasada cuyo IVA caería en un periodo ya presentado o cuyo plazo
/// terminó ([`late_vat`]). Lo decide el usuario: no se hace en silencio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LateVat {
    /// Con la fecha de la operación del periodo: su IVA es de aquel periodo, que habrá que
    /// corregir (autoliquidación rectificativa de aquel 303; con recargo si sale a ingresar,
    /// art. 27 LGT).
    Period,
    /// Sin fecha de la operación: el IVA se devenga al emitirla y va al periodo en curso. Sólo
    /// si el precio no era exigible antes (art. 75.Uno.7.º LIVA: en las operaciones de tracto
    /// sucesivo, el devengo es cuando resulta exigible cada parte del precio).
    Today,
}

/// Si la factura atrasada `d` (de [`preview_draft`]) lleva su IVA a un periodo con el 303 ya
/// presentado o fuera de plazo, por qué (como al anular una factura). Entonces hay que elegir
/// un [`LateVat`] para emitirla.
pub fn late_vat(v: &Vault, d: &InvoiceDraft, today: Date) -> Result<Option<String>> {
    if d.operation_date.is_none() {
        return Ok(None);
    }
    let s = invoicing::require_settings(v)?;
    let b = invoicing::build(&s, d, "BORRADOR".into())?;
    let accrual = crate::models::vat_accrual_date(&b.invoice, &b.totals.breakdown);
    crate::models::vat_period_closed(v, accrual, today)
}

/// Emite la factura que toca de la plantilla (con la confirmación del usuario) y pasa a la
/// siguiente.
pub fn issue(
    v: &Vault,
    id: &str,
    today: Date,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<IssuedInvoice> {
    issue_with(v, id, today, now, confirmed, None, None)
}

/// [`issue`] de una plantilla en moneda extranjera con el tipo de cambio vigente al devengo
/// (`currency`, de la misma moneda): el de la plantilla es el de la factura anterior y la
/// emisión no admite un tipo de otra fecha (art. 79.Once LIVA). La plantilla se queda con el
/// tipo nuevo.
pub fn issue_at_rate(
    v: &Vault,
    id: &str,
    today: Date,
    now: OffsetDateTime,
    confirmed: bool,
    currency: Option<ForeignCurrency>,
) -> Result<IssuedInvoice> {
    issue_with(v, id, today, now, confirmed, currency, None)
}

/// [`issue_at_rate`] con la decisión del usuario si la factura es de un periodo de IVA ya
/// cerrado ([`late_vat`]); sin ella, en ese caso no se emite.
///
/// La plantilla avanza en la misma transacción que la emisión: si algo falla no queda una
/// factura emitida con la plantilla sin avanzar (que al reintentar se emitiría dos veces).
pub fn issue_with(
    v: &Vault,
    id: &str,
    today: Date,
    now: OffsetDateTime,
    confirmed: bool,
    currency: Option<ForeignCurrency>,
    late: Option<LateVat>,
) -> Result<IssuedInvoice> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let (mut draft, language, period) = preview_period(v, id, today)?;
    if let Some(c) = currency {
        match &draft.currency {
            None => {
                return Err(AppError::Other(
                    "la plantilla es en euros: no lleva tipo de cambio".into(),
                ));
            }
            Some(old) if old.code != c.code => {
                return Err(AppError::Other(format!(
                    "la plantilla es en {}: el tipo debe ser de esa moneda",
                    old.code
                )));
            }
            Some(_) => draft.currency = Some(c),
        }
    }
    if draft.issue_date > today {
        return Err(AppError::Other(format!(
            "aún no toca: la siguiente es del {}",
            draft.issue_date
        )));
    }
    // Periodo de IVA ya cerrado: lo que haya elegido el usuario (queda en la auditoría).
    let late = match late_vat(v, &draft, today)? {
        None => None,
        Some(why) => match late {
            None => {
                return Err(AppError::Other(format!(
                    "Esta factura es de un periodo atrasado: {why}. Elige si la emites con la \
                     fecha de la operación de aquel periodo (y corriges aquel 303) o sin ella \
                     (su IVA va al periodo actual)."
                )));
            }
            Some(LateVat::Period) => Some(LateVat::Period),
            Some(LateVat::Today) => {
                draft.operation_date = None;
                Some(LateVat::Today)
            }
        },
    };
    invoicing::issue_linked(v, &draft, language, now, true, |conn, invoice_id| {
        let number: String = conn.query_row(
            "SELECT number FROM invoice_issued WHERE id = ?1",
            [invoice_id],
            |r| r.get(0),
        )?;
        let mut list = load(v)?;
        let t = list
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(AppError::NotFound)?;
        // Una sola factura por periodo, aunque se pulse dos veces.
        if t.next_date() != Some(period) {
            return Err(AppError::Other(
                "la plantilla ha cambiado mientras tanto: vuelve a abrirla".into(),
            ));
        }
        t.issued += 1;
        t.last = Some((number.clone(), draft.issue_date));
        t.draft.currency.clone_from(&draft.currency);
        store(v, &list)?;
        audit::append(
            conn,
            &now_str(now),
            Actor::User,
            "invoice_template.issue",
            id,
            &serde_json::json!({
                "number": number,
                "periodo": period.to_string(),
                "iva_atrasado": late,
            }),
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn meses_sin_arrastrar_el_dia() {
        let d = date!(2026 - 01 - 31);
        assert_eq!(months_after(d, 1), date!(2026 - 02 - 28));
        assert_eq!(months_after(d, 2), date!(2026 - 03 - 31));
        assert_eq!(months_after(d, 13), date!(2027 - 02 - 28));
        assert_eq!(
            months_after(date!(2024 - 01 - 31), 1),
            date!(2024 - 02 - 29)
        );
    }

    /// Si lo que va con la emisión (avanzar la plantilla) falla, no queda factura emitida.
    #[test]
    fn plantilla_y_factura_en_la_misma_transaccion() {
        use std::sync::Arc;
        use taxpain_core::invoice::{InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
        use taxpain_core::money::{Money, Rate};
        let v = Vault::open_in_memory(
            Arc::new(taxpain_vault::MemoryKeyProvider::default()),
            Arc::new(taxpain_vault::AlwaysConfirm),
        )
        .unwrap();
        let now = time::macros::datetime!(2026-10-01 10:00 UTC);
        let party = |nif: &str| Party {
            id: PartyId::Nif(nif.into()),
            name: "Prueba".into(),
            address: Some(taxpain_core::invoice::Address {
                street: "Calle Inventada 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
            person: None,
        };
        let s = crate::settings::InvoicingSettings {
            issuer: party("B12345674"),
            default_series: "A".into(),
            rectifying_series: "R".into(),
            simplified_series: "S".into(),
            simplified_rectifying_series: "RS".into(),
            language: Language::Es,
            default_vat: Rate::percent(21),
            default_withholding: None,
            outputs: crate::settings::Outputs {
                pdf: true,
                facturae: false,
                ubl: false,
            },
            destination: None,
            rounding: RoundingMode::PerRate,
            verifactu: crate::settings::VerifactuSettings {
                enabled: true,
                installation_id: "TP-test".into(),
                production: false,
            },
            footer: None,
            activity: None,
            simplified_up_to_3000: false,
        };
        invoicing::save_settings(&v, &s, now).unwrap();
        let d = InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 10 - 01),
            operation_date: None,
            recipient: Some(party("12345678Z")),
            lines: vec![InvoiceLine {
                description: "Cuota".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(100, 0),
                discount: Rate::ZERO,
                vat: VatTreatment::Taxed {
                    rate: Rate::percent(21),
                },
                surcharge: None,
                withholding: None,
                advance: false,
            }],
            rectified: vec![],
            rectified_amounts: None,
            description: "Cuota".into(),
            notes: None,
            currency: None,
            oss: None,
        };
        let err = invoicing::issue_linked(&v, &d, None, now, true, |_, _| {
            Err(AppError::Other("no se pudo guardar la plantilla".into()))
        });
        assert!(err.is_err());
        let count = |sql: &str| -> i64 { v.connection().query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM invoice_issued"), 0);
        assert_eq!(count("SELECT count(*) FROM verifactu_record"), 0);
        // Y la siguiente sale con el primer número.
        let ok = invoicing::issue_linked(&v, &d, None, now, true, |_, _| Ok(())).unwrap();
        assert!(ok.number.ends_with('1'), "{}", ok.number);
    }

    #[test]
    fn marcadores() {
        let d = date!(2026 - 10 - 01);
        assert_eq!(
            fill("Cuota {mes} ({trimestre}, {año})", d, Language::Es),
            "Cuota octubre 2026 (4T 2026, 2026)"
        );
        assert_eq!(fill("Fee {month}", d, Language::En), "Fee October 2026");
    }
}
