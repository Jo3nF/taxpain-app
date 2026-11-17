//! Confirmar, corregir o deshacer la revisión de una factura recibida.
//!
//! - **Antes de confirmar** una pendiente, [`comprobar`] valida los datos igual que la revisión y
//!   devuelve el resumen que se enseña para confirmarlos: proveedor, importes en euros y el
//!   trimestre del 303 al que va su IVA (el de la fecha de registro, art. 99.Tres LIVA).
//! - **Corregir** una revisada ([`corregir`]): cambia sus datos (también la fecha y el desglose) y
//!   rehace su asiento **en su sitio** ([`crate::accounting::rewrite_entry`]): mismo asiento, con
//!   sus adjuntos y observaciones; las líneas que siguen conservan el reparto analítico y, si no
//!   cambia su importe, el punteo. Es lo que piden los arts. 62-64 y 70 RIVA para rectificar una
//!   anotación del libro registro mal hecha. La fecha de registro no cambia: sigue en su 303.
//!   Queda en la auditoría con el antes y el después.
//! - **Deshacer la revisión** ([`deshacer`]): vuelve a «pendiente de revisar» y borra su asiento,
//!   si no tiene pagos ni conciliación.
//!
//! No se hace (error con el motivo): en un ejercicio cerrado (hay que reabrirlo desde Cierre,
//! como en el resto de la app); en bienes de inversión (están dados de alta en el inmovilizado);
//! cuando el asiento lo usa otra cosa (la cuota de un leasing se corrige desde Leasing); y, si
//! ya está pagada, lo que rompería el pago (el total, el proveedor, la moneda o el criterio de
//! caja).
//!
//! Se avisa (y no se presenta nada): si un 303 —u otro modelo afectado: 111, 115, 349, 347— de su
//! periodo ya está presentado o lo tiene la AEAT, y si la liquidación del IVA del trimestre ya
//! está contabilizada (hay que rehacerla).

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_invoicing::ingest::{Confidence, ExtractedFx, Extraction, Field};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::received::{self, Checked, ReceivedFx, Reviewed, Stored, VatRegime};

/// Lo que se enseña de una recibida para confirmar (importes en euros).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub proveedor_nif: String,
    pub proveedor: String,
    pub numero: String,
    pub fecha: Date,
    pub base: Money,
    pub iva: Money,
    pub retencion: Money,
    pub total: Money,
    pub deducible_pct: u8,
    pub regimen: VatRegime,
    /// En moneda extranjera: su código y el total en ella.
    pub moneda: Option<String>,
    pub total_moneda: Option<Money>,
}

impl Resumen {
    fn of(c: &Checked, fx: Option<&ReceivedFx>) -> Resumen {
        Resumen::from_reviewed(&c.r, &c.supplier, fx)
    }

    fn from_reviewed(r: &Reviewed, supplier: &str, fx: Option<&ReceivedFx>) -> Resumen {
        Resumen {
            proveedor_nif: supplier.to_owned(),
            proveedor: r.supplier_name.trim().to_owned(),
            numero: r.number.trim().to_owned(),
            fecha: r.issue_date,
            base: r.base,
            iva: r.vat,
            retencion: r.withholding,
            total: r.total,
            deducible_pct: r.deductible_pct,
            regimen: r.vat_regime,
            moneda: fx.map(|f| f.currency.code.clone()),
            total_moneda: fx.map(|f| f.total),
        }
    }
}

/// Resultado de [`comprobar`]: lo que quedará y lo que conviene saber antes de confirmarlo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Comprobacion {
    /// `true`: es una corrección de una revisada; `false`, la primera revisión.
    pub correccion: bool,
    /// Cómo queda.
    pub despues: Resumen,
    /// Cómo estaba (sólo al corregir).
    pub antes: Option<Resumen>,
    /// Trimestre del 303 al que va su IVA soportado («3T 2026»): el de la fecha de registro.
    pub trimestre: String,
    /// Avisos para leer antes de confirmar.
    pub avisos: Vec<String>,
    /// Modelos ya presentados que la factura toca y que se pueden rectificar desde Modelos
    /// («Preparar la rectificativa del 303 de ese trimestre»).
    pub rectificar: Vec<crate::rectificativa::Rectificable>,
}

/// Resultado de [`corregir`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Corregida {
    pub avisos: Vec<String>,
    /// Como en [`Comprobacion::rectificar`].
    pub rectificar: Vec<crate::rectificativa::Rectificable>,
    /// El asiento pasó de ejercicio (la nueva fecha es de otro): el de antes.
    pub ejercicio_anterior: Option<i32>,
    /// Líneas del asiento que estaban punteadas y cambiaron de importe (se les quita el punteo).
    pub punteos_quitados: usize,
}

/// (año, trimestre) de una fecha.
fn quarter(d: Date) -> (i32, u8) {
    (d.year(), (u8::from(d.month()) - 1) / 3 + 1)
}

fn label((y, q): (i32, u8)) -> String {
    format!("{q}T {y}")
}

/// Periodo («3T», «07» o «0A») en que el modelo `model` de la fecha `d` ya está presentado:
/// desde TaxPain (en producción), importado en «Modelos presentados» o según la última
/// comprobación de lo presentado en la AEAT (también lo que presentó la gestoría).
fn presented(v: &Vault, model: &str, d: Date) -> Result<Option<String>> {
    let month = u8::from(d.month());
    let periods = if model == "347" {
        vec!["0A".to_owned()]
    } else {
        vec![format!("{}T", quarter(d).1), format!("{month:02}")]
    };
    let conn = v.connection();
    for p in periods {
        let local: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tax_return WHERE model = ?1 AND year = ?2
                             AND period = ?3 AND environment = 'produccion')
                 OR EXISTS(SELECT 1 FROM filed_model WHERE model = ?1 AND year = ?2 AND period = ?3)",
            params![model, d.year(), p],
            |r| r.get(0),
        )?;
        if local || crate::presentado_aeat::filed_at_aeat(v, model, d.year(), &p)? {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

/// Texto del periodo presentado: «3T de 2026», «07/2026» o «2026».
fn period_text(p: &str, year: i32) -> String {
    match p {
        "0A" => year.to_string(),
        p if p.ends_with('T') => format!("{p} de {year}"),
        p => format!("{p}/{year}"),
    }
}

/// Aviso por un 303 presentado. TODO(VERIFY): el reparto entre rectificativa (si baja la
/// deducción o sube el devengado) y periodo posterior dentro de los cuatro años (si sube la
/// deducción) sale de los arts. 99.Tres y 114 LIVA; que la autoliquidación rectificativa sustituye
/// a la complementaria, del art. 120.3 LGT y el RD 117/2024. Comprobarlo con la redacción vigente.
fn aviso_303(p: &str, year: i32) -> String {
    format!(
        "El 303 del {} ya está presentado (o la AEAT lo tiene): TaxPain no presenta nada. Si la \
         corrección baja el IVA deducible o sube el devengado, hay que presentar una \
         autoliquidación rectificativa de ese 303; si sube el IVA deducible, la diferencia puede \
         deducirse en un 303 posterior, dentro de los cuatro años (arts. 99.Tres y 114 LIVA). \
         Compara después lo presentado con lo calculado en Modelos.",
        period_text(p, year)
    )
}

/// Fechas de los pagos de la recibida: del banco (conciliados) y de Cartera.
fn payments(conn: &Connection, id: i64) -> Result<(Vec<Date>, Vec<Date>)> {
    let read = |sql: &str| -> Result<Vec<Date>> {
        let mut st = conn.prepare(sql)?;
        let rows = st.query_map([id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for d in rows {
            let d = d?;
            if let Ok(d) = Date::parse(
                d.get(..10).unwrap_or(&d),
                time::macros::format_description!("[year]-[month]-[day]"),
            ) {
                out.push(d);
            }
        }
        out.sort();
        Ok(out)
    };
    Ok((
        read(
            "SELECT op_date FROM bank_tx WHERE matched_kind = 'recibida' AND matched_id = ?1
              ORDER BY op_date",
        )?,
        read("SELECT date FROM settlement WHERE kind = 'recibida' AND invoice_id = ?1")?,
    ))
}

/// Asiento de la recibida (`None` si no tiene: la del libro del gestor).
fn entry_of(conn: &Connection, id: i64) -> Result<Option<(i64, i32)>> {
    Ok(conn
        .query_row(
            "SELECT id, fiscal_year FROM journal_entry
              WHERE origin_kind = 'FacturaRecibida' AND origin_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Error si el ejercicio de `date` (o el número `year`) está cerrado.
fn check_open(v: &Vault, date: Option<Date>, year: Option<i32>) -> Result<()> {
    let conn = v.connection();
    let year = match (year, date) {
        (Some(y), _) => y,
        (None, Some(d)) => crate::accounting::fiscal_year_at(v, d)?.year,
        (None, None) => return Ok(()),
    };
    if crate::accounting::year_closed(conn, year)? {
        return Err(AppError::Other(format!(
            "el ejercicio {year} está cerrado: para cambiar esta factura hay que reabrirlo \
             desde Cierre"
        )));
    }
    Ok(())
}

/// Error si otra cosa usa el asiento de la recibida (la cuota de un leasing, un gasto de viaje…).
fn check_references(conn: &Connection, entry: i64) -> Result<()> {
    for (table, column, why) in crate::import::ENTRY_REFERENCES {
        let used: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
            [entry],
            |r| r.get(0),
        )?;
        if used {
            return Err(AppError::Other(format!(
                "su asiento {why}: se cambia desde allí"
            )));
        }
    }
    Ok(())
}

/// Las recibidas de bienes de inversión no se corrigen ni se deshacen aquí: su alta en el
/// inmovilizado (y su amortización) no va enlazada a la factura.
fn check_not_capital(r: &Reviewed) -> Result<()> {
    if r.capital_good {
        return Err(AppError::Other(
            "es un bien de inversión dado de alta en el inmovilizado: corrígelo desde \
             Inmovilizado (o dalo de baja y registra la factura de nuevo)"
                .into(),
        ));
    }
    Ok(())
}

/// Devengo del IVA autorrepercutido (intracomunitarias e ISP): la menor de la fecha de la
/// factura y la de registro (como `models::self_assessed_accrual`).
fn accrual(r: &Reviewed, registered: Date) -> Option<Date> {
    (r.vat_regime != VatRegime::Interior).then(|| r.issue_date.min(registered))
}

/// Avisos de los periodos presentados o liquidados que toca una recibida con los datos `rs`
/// (los de antes y los de después, al corregir) registrada el día `registered`.
/// `iva`, `ret`, `m349` y `m347`: si cambia lo que va a cada uno.
#[allow(clippy::too_many_arguments)]
fn period_warnings(
    v: &Vault,
    rs: &[&Reviewed],
    registered: Date,
    bank_paid: Option<Date>,
    cash_paid: &[Date],
    iva: bool,
    ret: bool,
    m349: bool,
    m347: bool,
) -> Result<Vec<String>> {
    Ok(period_warnings_links(
        v, rs, registered, bank_paid, cash_paid, iva, ret, m349, m347,
    )?
    .0)
}

/// Un modelo trimestral presentado que se puede rectificar desde Modelos (sólo los
/// trimestrales: el borrador mensual no lo hace TaxPain).
fn rectificable(
    model: &str,
    period: &str,
    year: i32,
) -> Option<crate::rectificativa::Rectificable> {
    let q = period.strip_suffix('T')?.parse().ok()?;
    Some(crate::rectificativa::Rectificable {
        modelo: model.to_owned(),
        ejercicio: year,
        trimestre: q,
    })
}

/// [`period_warnings`] con los modelos presentados que se pueden rectificar.
#[allow(clippy::too_many_arguments)]
fn period_warnings_links(
    v: &Vault,
    rs: &[&Reviewed],
    registered: Date,
    bank_paid: Option<Date>,
    cash_paid: &[Date],
    iva: bool,
    ret: bool,
    m349: bool,
    m347: bool,
) -> Result<(Vec<String>, Vec<crate::rectificativa::Rectificable>)> {
    let conn = v.connection();
    let mut out = Vec::new();
    let mut links = Vec::new();
    if iva {
        // El IVA soportado va al trimestre de registro; el autorrepercutido, al del devengo.
        // Con criterio de caja, al del pago (art. 163 terdecies LIVA).
        let mut dates = vec![registered];
        dates.extend(rs.iter().filter_map(|r| accrual(r, registered)));
        if rs.iter().any(|r| r.cash_basis) {
            dates.extend(cash_paid);
        }
        let mut seen = Vec::new();
        for d in dates {
            let q = quarter(d);
            if seen.contains(&q) {
                continue;
            }
            seen.push(q);
            if let Some(p) = presented(v, "303", d)? {
                out.push(aviso_303(&p, d.year()));
                links.extend(rectificable("303", &p, d.year()));
                continue;
            }
            let settled: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM journal_entry
                                WHERE origin_kind = 'LiquidacionIva' AND origin_id = ?1)",
                [i64::from(q.0) * 100 + i64::from(q.1)],
                |r| r.get(0),
            )?;
            if settled {
                out.push(format!(
                    "La liquidación del IVA del {} ya está en el diario: después, rehazla en \
                     Modelos («Rehacer la liquidación») para que la 472 y la 477 cuadren con el \
                     303.",
                    label(q)
                ));
            }
        }
    }
    if ret {
        // Retenciones: en el trimestre del pago (art. 78.1 RIRPF) o, sin pago, el de la factura.
        let mut seen = Vec::new();
        for r in rs {
            if r.withholding.is_zero() {
                continue;
            }
            let d = bank_paid.unwrap_or(r.issue_date);
            let model = if r.nature == received::Nature::Alquiler {
                "115"
            } else {
                "111"
            };
            if seen.contains(&(model, quarter(d))) {
                continue;
            }
            seen.push((model, quarter(d)));
            if let Some(p) = presented(v, model, d)? {
                out.push(format!(
                    "El {model} del {} ya está presentado: si cambia la retención, presenta una \
                     complementaria o rectificativa de ese {model} (y revisa el resumen anual).",
                    period_text(&p, d.year())
                ));
                links.extend(rectificable(model, &p, d.year()));
            }
        }
    }
    if m349 {
        let mut seen = Vec::new();
        for r in rs
            .iter()
            .filter(|r| r.vat_regime == VatRegime::Intracomunitaria)
        {
            let d = accrual(r, registered).unwrap_or(r.issue_date);
            if seen.contains(&quarter(d)) {
                continue;
            }
            seen.push(quarter(d));
            if let Some(p) = presented(v, "349", d)? {
                out.push(format!(
                    "El 349 del {} ya está presentado: la adquisición intracomunitaria cambia; \
                     hay que rectificarlo en la Sede.",
                    period_text(&p, d.year())
                ));
            }
        }
    }
    if m347 && let Some(p) = presented(v, "347", registered)? {
        out.push(format!(
            "El 347 de {} ya está presentado: si con la corrección cambia lo que suma este \
             proveedor (o pasa de 3.005,06 €), hay que rectificarlo en la Sede.",
            period_text(&p, registered.year())
        ));
    }
    Ok((out, links))
}

/// Comprueba los datos de la recibida `id` antes de confirmarlos y devuelve el resumen y los
/// avisos. Pendiente: los de la primera revisión. Revisada: los de su corrección, con lo que la
/// impide como error (ver el módulo).
pub fn comprobar(
    v: &Vault,
    id: i64,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    now: OffsetDateTime,
) -> Result<Comprobacion> {
    let conn = v.connection();
    let (status, old) = received::stored(conn, id)?;
    match status.as_str() {
        "pendiente" => check_review(v, id, r, fx, now),
        "revisada" => Ok(check_correction(v, id, &old, r, fx)?.0),
        _ => Err(AppError::Other("la factura está descartada".into())),
    }
}

/// [`comprobar`] de la primera revisión.
fn check_review(
    v: &Vault,
    id: i64,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    now: OffsetDateTime,
) -> Result<Comprobacion> {
    let c = received::check_reviewed(v, id, r, fx)?;
    check_open(v, Some(c.r.issue_date), None)?;
    let registered = taxpain_core::spain_time::peninsular_date(now);
    let mut avisos = Vec::new();
    if quarter(c.r.issue_date) < quarter(registered) && c.r.vat_regime == VatRegime::Interior {
        avisos.push(format!(
            "La factura es del {} pero se registra hoy: su IVA se deduce en el 303 del {} (art. \
             99.Tres LIVA). Si ya la tenías y querías deducirla en su trimestre, ese 303 hay que \
             rectificarlo.",
            label(quarter(c.r.issue_date)),
            label(quarter(registered))
        ));
    }
    let (textos, rectificar) =
        period_warnings_links(v, &[&c.r], registered, None, &[], true, false, false, false)?;
    avisos.extend(textos);
    Ok(Comprobacion {
        correccion: false,
        despues: Resumen::of(&c, fx),
        antes: None,
        trimestre: label(quarter(registered)),
        avisos,
        rectificar,
    })
}

/// Lo que ha cambiado entre los datos de antes y los de después.
#[derive(Clone, Copy, Debug)]
struct Changes {
    any: bool,
    iva: bool,
    ret: bool,
    m349: bool,
    m347: bool,
    /// Lo que rompería un pago: el total, el proveedor, la moneda o el criterio de caja.
    payment: bool,
    total_or_date: bool,
}

fn changes(old: &Stored, new: &Checked, new_fx: Option<&ReceivedFx>) -> Changes {
    let a = &old.reviewed;
    // Los datos nuevos como se guardan (NIF normalizado, marcas que sólo valen en su régimen…),
    // para compararlos con los guardados.
    let b = &Reviewed {
        supplier_nif: new.supplier.clone(),
        supplier_name: new.r.supplier_name.trim().to_owned(),
        number: new.r.number.trim().to_owned(),
        supplier_country: new.country.clone(),
        self_assessed_rate: new.rate,
        intra_goods: new.r.vat_regime == VatRegime::Intracomunitaria && new.r.intra_goods,
        cash_basis: new.r.vat_regime == VatRegime::Interior && new.r.cash_basis,
        import_dua: new.dua,
        asset_class: a.asset_class,
        ..new.r.clone()
    };
    let self_assessed = a.vat_regime != VatRegime::Interior || b.vat_regime != VatRegime::Interior;
    let nif = a.supplier_nif != b.supplier_nif;
    let supplier = nif || a.supplier_name != b.supplier_name;
    // Lo que cambia el 303 (el nombre o el número del proveedor, no).
    let iva = a.base != b.base
        || a.vat != b.vat
        || a.deductible_pct != b.deductible_pct
        || a.vat_regime != b.vat_regime
        || a.self_assessed_rate != b.self_assessed_rate
        || a.cash_basis != b.cash_basis
        || a.import_dua != b.import_dua
        || a.prorata_use != b.prorata_use
        || a.intra_goods != b.intra_goods
        || a.supplier_country != b.supplier_country
        || nif
        || (self_assessed && a.issue_date != b.issue_date);
    let ret = a.withholding != b.withholding
        || a.nature != b.nature
        || (!a.withholding.is_zero() && (supplier || a.issue_date != b.issue_date));
    let intra =
        a.vat_regime == VatRegime::Intracomunitaria || b.vat_regime == VatRegime::Intracomunitaria;
    let m349 = intra
        && (a.vat_regime != b.vat_regime
            || a.base != b.base
            || supplier
            || a.intra_goods != b.intra_goods
            || a.issue_date != b.issue_date);
    let m347 = supplier || a.total != b.total;
    let fx_amount = |f: Option<&ReceivedFx>| f.map(|f| (f.currency.code.clone(), f.total));
    let payment = a.total != b.total
        || nif
        || fx_amount(old.fx.as_ref()) != fx_amount(new_fx)
        || a.cash_basis != b.cash_basis;
    Changes {
        any: a != b || old.fx.as_ref() != new_fx,
        iva,
        ret,
        m349,
        m347,
        payment,
        total_or_date: a.total != b.total || a.issue_date != b.issue_date,
    }
}

/// [`comprobar`] de una corrección: el resumen, los datos comprobados y lo que ha cambiado.
fn check_correction(
    v: &Vault,
    id: i64,
    old: &Stored,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
) -> Result<(Comprobacion, Checked, Changes)> {
    let conn = v.connection();
    check_not_capital(&old.reviewed)?;
    if r.capital_good {
        return Err(AppError::Other(
            "para pasarla a bien de inversión, deshaz la revisión y revísala de nuevo marcándolo"
                .into(),
        ));
    }
    let c = received::check_reviewed(v, id, r, fx)?;
    let entry = entry_of(conn, id)?;
    if let Some((e, year)) = entry {
        check_references(conn, e)?;
        check_open(v, None, Some(year))?;
    } else {
        check_open(v, Some(old.reviewed.issue_date), None)?;
    }
    check_open(v, Some(c.r.issue_date), None)?;
    let ch = changes(old, &c, fx);
    if !ch.any {
        return Err(AppError::Other("no has cambiado nada".into()));
    }
    let (bank, cartera) = payments(conn, id)?;
    if ch.payment && !(bank.is_empty() && cartera.is_empty()) {
        return Err(AppError::Other(format!(
            "ya está pagada ({}): para cambiar el total, el proveedor, la moneda o el criterio de \
             caja, deshaz antes el pago; lo demás (fecha, desglose, deducible…) sí se corrige",
            if bank.is_empty() {
                "registrado en Cartera"
            } else {
                "conciliada con el banco"
            }
        )));
    }
    let registered = old.registered_on.unwrap_or(old.reviewed.issue_date);
    let paid: Vec<Date> = bank.iter().chain(&cartera).copied().collect();
    let (mut avisos, rectificar) = period_warnings_links(
        v,
        &[&old.reviewed, &c.r],
        registered,
        bank.first().copied(),
        &paid,
        ch.iva,
        ch.ret,
        ch.m349,
        ch.m347,
    )?;
    if let Some((_, year)) = entry {
        let new_year = crate::accounting::fiscal_year_at(v, c.r.issue_date)?.year;
        if new_year != year {
            avisos.push(format!(
                "Con la nueva fecha, el asiento pasa del ejercicio {year} al {new_year}."
            ));
        }
    } else {
        avisos.push(
            "Viene del libro del gestor y no tiene asiento en TaxPain: sólo cambian el libro \
             registro y los modelos."
                .into(),
        );
    }
    if !ch.payment && !(bank.is_empty() && cartera.is_empty()) {
        avisos
            .push("Está pagada: el pago no cambia (el total y el proveedor siguen igual).".into());
    }
    Ok((
        Comprobacion {
            correccion: true,
            despues: Resumen::of(&c, fx),
            antes: Some(Resumen::from_reviewed(
                &old.reviewed,
                &old.reviewed.supplier_nif,
                old.fx.as_ref(),
            )),
            trimestre: label(quarter(registered)),
            avisos,
            rectificar,
        },
        c,
        ch,
    ))
}

/// Corrige una recibida ya revisada con los datos `r` (y `fx`, si es en moneda extranjera):
/// guarda los datos nuevos, rehace su asiento en su sitio y lo deja en la auditoría con el antes
/// y el después. La fecha de registro se conserva. Lo que lo impide sale como error (ver
/// [`comprobar`]).
pub fn corregir(
    v: &Vault,
    id: i64,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    now: OffsetDateTime,
) -> Result<Corregida> {
    let conn = v.connection();
    let (status, old) = received::stored(conn, id)?;
    if status != "revisada" {
        return Err(AppError::Other(
            "sólo se corrigen las facturas revisadas: ésta se revisa con «Confirmar datos»".into(),
        ));
    }
    let (check, c, ch) = check_correction(v, id, &old, r, fx)?;
    let tx = conn.unchecked_transaction()?;
    if received::update_row(&tx, id, &c, None, "revisada")? == 0 {
        return Err(AppError::Other(
            "la factura ha cambiado mientras: vuelve a abrirla".into(),
        ));
    }
    let mut rewritten = crate::accounting::Rewritten::default();
    let mut after_entry = None;
    if let Some((entry, _)) = entry_of(&tx, id)? {
        let e = crate::accounting::received_invoice_entry(&tx, &c.invoice(id), &c.supplier, None)?;
        rewritten = crate::accounting::rewrite_entry(&tx, entry, &e)?;
        after_entry = Some(e);
    }
    // Sin pagos (si los hay, el total no cambia): los vencimientos se recalculan con el total y
    // la fecha nuevos.
    if ch.total_or_date {
        tx.execute(
            "DELETE FROM invoice_installment WHERE kind = 'recibida' AND invoice_id = ?1
               AND NOT EXISTS(SELECT 1 FROM settlement WHERE kind = 'recibida' AND invoice_id = ?1)
               AND NOT EXISTS(SELECT 1 FROM bank_tx WHERE matched_kind = 'recibida'
                                AND matched_id = ?1)",
            [id],
        )?;
    }
    audit::append(
        &tx,
        &crate::invoicing::now_str(now),
        Actor::User,
        "received.correct",
        c.r.number.trim(),
        &serde_json::json!({
            "id": id,
            "before": { "reviewed": old.reviewed, "fx": old.fx },
            "after": { "reviewed": c.r, "fx": fx },
            "entry_before": rewritten.before,
            "entry_after": after_entry,
            "moved_from_year": rewritten.moved_from_year,
            "unticked": rewritten.unticked,
            "warnings": check.avisos,
        }),
    )?;
    tx.commit()?;
    Ok(Corregida {
        avisos: check.avisos,
        rectificar: check.rectificar,
        ejercicio_anterior: rewritten.moved_from_year,
        punteos_quitados: rewritten.unticked,
    })
}

/// Lo que impide deshacer la revisión de la recibida `id` (error) o conviene saber (avisos).
pub fn comprobar_deshacer(v: &Vault, id: i64) -> Result<Vec<String>> {
    let conn = v.connection();
    let (status, old) = received::stored(conn, id)?;
    if status != "revisada" {
        return Err(AppError::Other("la factura no está revisada".into()));
    }
    check_undo(v, id, &old)
}

fn check_undo(v: &Vault, id: i64, old: &Stored) -> Result<Vec<String>> {
    let conn = v.connection();
    check_not_capital(&old.reviewed)?;
    if !old.posted {
        return Err(AppError::Other(
            "viene del libro del gestor (ya está en su contabilidad): corrígela con «Corregir…»"
                .into(),
        ));
    }
    let (bank, cartera) = payments(conn, id)?;
    if !bank.is_empty() {
        return Err(AppError::Other(
            "está conciliada con el banco: deshaz antes la conciliación del pago".into(),
        ));
    }
    if !cartera.is_empty() {
        return Err(AppError::Other(
            "tiene el pago registrado en Cartera: deshazlo antes".into(),
        ));
    }
    if let Some((entry, year)) = entry_of(conn, id)? {
        check_references(conn, entry)?;
        check_open(v, None, Some(year))?;
        let (docs, notes): (i64, Option<String>) = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM entry_document WHERE entry_id = ?1), notes
               FROM journal_entry WHERE id = ?1",
            [entry],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if docs > 0 || notes.is_some_and(|n| !n.trim().is_empty()) {
            return Err(AppError::Other(
                "su asiento tiene documentos u observaciones en el Diario: guárdalos y quítalos \
                 antes (se borrarían con él)"
                    .into(),
            ));
        }
    }
    let registered = old.registered_on.unwrap_or(old.reviewed.issue_date);
    let r = &old.reviewed;
    let mut avisos = period_warnings(
        v,
        &[r],
        registered,
        None,
        &[],
        true,
        !r.withholding.is_zero(),
        r.vat_regime == VatRegime::Intracomunitaria,
        true,
    )?;
    avisos.push(format!(
        "Vuelve a «Pendientes de revisar» y se borra su asiento: deja de contar en el 303 del {} \
         y en el libro registro. Si la revisas otra vez, irá al trimestre en que la revises.",
        label(quarter(registered))
    ));
    Ok(avisos)
}

/// Deshace la revisión de la recibida `id`: vuelve a «pendiente de revisar» con los datos que se
/// confirmaron como propuesta, se borran su asiento y sus vencimientos, y queda en la auditoría.
/// Sólo si no tiene pagos ni conciliación (ver [`comprobar_deshacer`]).
pub fn deshacer(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let (status, old) = received::stored(conn, id)?;
    if status != "revisada" {
        return Err(AppError::Other("la factura no está revisada".into()));
    }
    let avisos = check_undo(v, id, &old)?;
    let extraction: String = conn.query_row(
        "SELECT extraction FROM invoice_received WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let mut e: Extraction = serde_json::from_str(&extraction).unwrap_or_default();
    proposal(&mut e, &old, now);
    let e_json = serde_json::to_string(&e).map_err(|err| AppError::Other(err.to_string()))?;
    let tx = conn.unchecked_transaction()?;
    let entry = entry_of(&tx, id)?;
    let deleted = match entry {
        Some((entry, _)) => {
            let before = crate::accounting::load_entry(&tx, entry)?;
            crate::accounting::delete_entry_rows(&tx, entry)?;
            Some(before)
        }
        None => None,
    };
    tx.execute(
        "DELETE FROM invoice_installment WHERE kind = 'recibida' AND invoice_id = ?1",
        [id],
    )?;
    let n = tx.execute(
        "UPDATE invoice_received SET status = 'pendiente', reviewed_at = NULL, extraction = ?2
          WHERE id = ?1 AND status = 'revisada'",
        params![id, e_json],
    )?;
    if n == 0 {
        return Err(AppError::Other(
            "la factura ha cambiado mientras: vuelve a abrirla".into(),
        ));
    }
    audit::append(
        &tx,
        &crate::invoicing::now_str(now),
        Actor::User,
        "received.unreview",
        old.reviewed.number.trim(),
        &serde_json::json!({
            "id": id,
            "before": { "reviewed": old.reviewed, "fx": old.fx,
                        "registered_on": old.registered_on.map(|d| d.to_string()) },
            "entry_deleted": deleted,
            "warnings": avisos,
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// La extracción de una recibida que vuelve a pendiente: los datos que se confirmaron (en la
/// moneda de la factura, si es extranjera), para revisarla otra vez sin teclearlos.
fn proposal(e: &mut Extraction, old: &Stored, now: OffsetDateTime) {
    fn sure<T>(value: T) -> Option<Field<T>> {
        Some(Field {
            value,
            confidence: Confidence::Alta,
        })
    }
    let r = &old.reviewed;
    e.supplier_nif = sure(r.supplier_nif.clone());
    e.supplier_name = sure(r.supplier_name.clone());
    e.number = sure(r.number.clone());
    e.issue_date = sure(r.issue_date);
    let (base, vat, withholding, total) = match &old.fx {
        Some(f) => (f.base, f.vat, f.withholding, f.total),
        None => (r.base, r.vat, r.withholding, r.total),
    };
    e.base = sure(base);
    e.vat = sure(vat);
    e.withholding = (!withholding.is_zero())
        .then_some(withholding)
        .and_then(sure);
    e.total = sure(total);
    e.fx = old.fx.as_ref().map(|f| ExtractedFx {
        code: f.currency.code.clone(),
        rate: Some(f.currency.rate.decimal()),
        rate_date: Some(f.currency.rate_date),
        vat_eur: f.vat_eur.or(Some(r.vat)),
    });
    let day = taxpain_core::spain_time::peninsular_date(now);
    e.warnings.push(format!(
        "revisión deshecha el {:02}/{:02}/{}: son los datos que se confirmaron; revisa también \
         el tipo de gasto, el régimen de IVA y el porcentaje deducible",
        day.day(),
        u8::from(day.month()),
        day.year()
    ));
}
