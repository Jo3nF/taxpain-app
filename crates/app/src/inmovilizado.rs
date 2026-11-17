//! Correcciones del inmovilizado e incentivos fiscales de su amortización.
//!
//! - **Editar un elemento**: nombre, fecha de puesta en funcionamiento (la del alta, desde la que
//!   se amortiza), coeficiente (vida útil) y valor residual. Las amortizaciones ya registradas no
//!   se tocan; las de los ejercicios sin registrar se calculan con los datos nuevos (un cambio de
//!   estimación se aplica de forma prospectiva: PGC, NRV 22.ª). La fecha no se cambia si ya hay
//!   amortizaciones registradas (cambiaría lo ya contabilizado): se deshacen antes.
//! - **Borrar un elemento** sin asientos (dado de alta por error).
//! - **Deshacer una baja o una venta**: se borra su asiento y la amortización del ejercicio vuelve
//!   a como estaba antes de la baja (la que se rehízo hasta ese día recupera su fecha y su cuota;
//!   la que creó la baja se quita; la de ejercicios posteriores que la baja quitó se vuelve a
//!   registrar), con lo que guarda la auditoría de la baja. Si tiene la factura de venta
//!   vinculada, se desvincula antes.
//! - **Deshacer la amortización de un ejercicio abierto** (de todos los elementos o de uno), si
//!   es la última registrada del elemento y no forma parte de una baja; y la **previsión** de lo
//!   que se va a registrar, para confirmarlo con el total.
//! - **Incentivos fiscales** (el plan fiscal va en el coeficiente fiscal del elemento y la
//!   diferencia con la contable es el ajuste del 200 que ya propone
//!   [`crate::accounting::depreciation_differences`], con su reversión en la baja):
//!   - Elementos de **escaso valor** (art. 12.3.e LIS, Ley 27/2014): libertad de amortización de
//!     los elementos nuevos del inmovilizado material de valor unitario hasta 300 €, con el límite
//!     de 25.000 € por periodo impositivo.
//!   - **Libertad de amortización con creación de empleo** de la empresa de reducida dimensión
//!     (art. 102 LIS): elementos nuevos del inmovilizado material y de las inversiones
//!     inmobiliarias, si en los 24 meses siguientes al inicio del periodo en que entran en
//!     funcionamiento la plantilla media total crece respecto de la de los 12 meses anteriores y
//!     el aumento se mantiene otros 24 meses; inversión máxima: 120.000 € por el incremento de
//!     plantilla calculado con dos decimales.
//!   - **Amortización acelerada** de la empresa de reducida dimensión (art. 103 LIS): hasta el
//!     doble del coeficiente lineal máximo de las tablas en los elementos nuevos del inmovilizado
//!     material, las inversiones inmobiliarias y el intangible.
//!   - Otra libertad de amortización (de otro precepto o de un régimen transitorio): sin topes
//!     que comprobar aquí.
//!
//!   La condición de empresa de reducida dimensión (art. 101 LIS: cifra de negocios del periodo
//!   anterior inferior a 10 M€, con la prórroga del 101.4) y que los elementos sean nuevos no se
//!   pueden comprobar del todo con los datos de la app: se avisa. TODO(VERIFY): el requisito de
//!   empleo del art. 102 (cálculo de la plantilla media, art. 102.1 y 2, y la regularización del
//!   art. 102.4 si no se cumple: ingresar la cuota de lo amortizado de más con intereses de
//!   demora) y el 150 % del art. 103.1 para el intangible de vida útil indefinida y el fondo de
//!   comercio (art. 12.2 y 12.3 LIS) se dejan al usuario; y las casillas propias de cada
//!   incentivo en el modelo 200 (la app propone la diferencia en las de diferencias de
//!   amortización).

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::JournalEntry;
use taxpain_core::accounting::depreciation::{FixedAsset, depreciation_entry_in};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::accounting::{
    delete_entry_rows, fiscal_year_conn, fiscal_year_for_date, fixed_assets, load_entry,
    move_attachments, record_entry, year_closed,
};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Valor unitario máximo de los elementos de escaso valor (art. 12.3.e LIS).
pub const ESCASO_VALOR_UNITARIO: Money = Money::from_euros(300, 0);
/// Límite anual de los elementos de escaso valor (art. 12.3.e LIS).
pub const ESCASO_VALOR_ANUAL: Money = Money::from_euros(25_000, 0);
/// Inversión máxima de la libertad de amortización con creación de empleo por cada persona de
/// incremento de plantilla (art. 102.2 LIS).
pub const ERD_EMPLEO_POR_PERSONA: Money = Money::from_euros(120_000, 0);
/// Cifra de negocios de la empresa de reducida dimensión (art. 101.1 LIS).
const DIEZ_MILLONES: Money = Money::from_euros(10_000_000, 0);

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn asset(v: &Vault, id: i64) -> Result<FixedAsset> {
    fixed_assets(v)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or(AppError::NotFound)
}

/// Asientos de amortización del elemento: (id, ejercicio, fecha, cuota).
fn posted(conn: &Connection, id: i64) -> Result<Vec<(i64, i32, Date, Money)>> {
    let mut st = conn.prepare(
        "SELECT e.id, e.fiscal_year, e.date,
                COALESCE((SELECT SUM(l.debit) FROM journal_line l WHERE l.entry_id = e.id), 0)
           FROM journal_entry e
          WHERE e.origin_kind = 'Amortizacion' AND e.origin_id = ?1
          ORDER BY e.date, e.id",
    )?;
    let rows = st.query_map([id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i32>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (e, y, d, c) = r?;
        out.push((e, y, parse_date(&d)?, Money::from_cents(c)));
    }
    Ok(out)
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

/// ¿Tiene el asiento documentos u observaciones (que se perderían al borrarlo)?
fn with_attachments(conn: &Connection, entry: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entry_document WHERE entry_id = ?1)
             OR COALESCE((SELECT trim(COALESCE(notes, '')) <> '' FROM journal_entry
                           WHERE id = ?1), 0)",
        [entry],
        |r| r.get(0),
    )?)
}

// ---------------- Editar y borrar ----------------

/// Datos de un elemento que se pueden cambiar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CambiosBien {
    pub name: String,
    /// Puesta en funcionamiento: desde ese día se amortiza.
    pub acquired_on: Date,
    /// Coeficiente anual (la vida útil es 100 / coeficiente).
    pub rate: Rate,
    pub residual: Money,
}

/// Cambia los datos de un elemento. Las amortizaciones registradas se quedan como están; las
/// de los ejercicios sin registrar salen con los datos nuevos. Devuelve lo que conviene saber.
pub fn edit_asset(v: &Vault, id: i64, c: &CambiosBien, now: OffsetDateTime) -> Result<Vec<String>> {
    let a = asset(v, id)?;
    let name = c.name.trim();
    if name.is_empty() {
        return Err(other("el elemento necesita un nombre"));
    }
    if c.rate.bp() <= 0 || c.rate.bp() > 10_000 {
        return Err(other("el coeficiente debe estar entre 0 y 100 %"));
    }
    if c.residual.is_negative() || c.residual >= a.cost {
        return Err(other(
            "el valor residual no puede ser negativo ni llegar al coste",
        ));
    }
    let datos = c.acquired_on != a.acquired_on || c.rate != a.rate || c.residual != a.residual;
    if a.disposed_on.is_some() && datos {
        return Err(other(
            "el elemento está dado de baja: deshaz la baja para cambiar sus datos",
        ));
    }
    let conn = v.connection();
    let hechas = posted(conn, id)?;
    let (prior, until): (i64, Option<String>) = conn.query_row(
        "SELECT prior_depreciation, prior_until FROM fixed_asset WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if c.acquired_on != a.acquired_on {
        if !hechas.is_empty() {
            let años: Vec<String> = hechas.iter().map(|h| h.1.to_string()).collect();
            return Err(other(format!(
                "«{}» ya tiene registrada la amortización de {}: cambiar la fecha cambiaría lo \
                 contabilizado; deshaz antes esas amortizaciones",
                a.name,
                años.join(", ")
            )));
        }
        if prior > 0 || until.is_some() {
            return Err(other(
                "el elemento viene de los saldos iniciales con su amortización de antes de \
                 TaxPain: su fecha se cambia en Saldos iniciales",
            ));
        }
    }
    let amortizado = hechas.iter().map(|h| h.3).sum::<Money>() + Money::from_cents(prior);
    if a.cost - c.residual < amortizado {
        return Err(other(format!(
            "ya hay {amortizado} amortizados: el valor residual no puede dejar menos por amortizar"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE fixed_asset SET name = ?1, acquired_on = ?2, rate_bp = ?3, residual = ?4
          WHERE id = ?5",
        params![
            name,
            c.acquired_on.to_string(),
            c.rate.bp(),
            c.residual.cents(),
            id
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.edit",
        &id.to_string(),
        &serde_json::json!({
            "before": {
                "name": a.name,
                "acquired_on": a.acquired_on.to_string(),
                "rate": a.rate.bp(),
                "residual": a.residual.cents(),
            },
            "after": {
                "name": name,
                "acquired_on": c.acquired_on.to_string(),
                "rate": c.rate.bp(),
                "residual": c.residual.cents(),
            },
        }),
    )?;
    tx.commit()?;
    let mut avisos = Vec::new();
    if datos && !hechas.is_empty() {
        let años: Vec<String> = hechas.iter().map(|h| h.1.to_string()).collect();
        avisos.push(format!(
            "La amortización ya registrada ({}) no cambia; la de los ejercicios siguientes se \
             calcula con los datos nuevos (un cambio de estimación vale hacia delante, NRV 22.ª \
             del PGC).",
            años.join(", ")
        ));
    }
    if c.rate != a.rate {
        avisos.push(format!(
            "Vida útil: {} años con el {} % anual. Si pasa del coeficiente máximo de las tablas \
             (art. 12.1.a LIS), el exceso no es deducible: pon el fiscal aparte en el plan.",
            taxpain_core::accounting::depreciation::useful_life(c.rate),
            c.rate
        ));
    }
    Ok(avisos)
}

/// Borra un elemento dado de alta por error: sólo si no tiene asientos (amortizaciones, baja),
/// arrendamiento ni amortización de antes de TaxPain.
pub fn delete_asset(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let a = asset(v, id)?;
    let conn = v.connection();
    let (asientos, leasing, prior): (i64, bool, bool) = conn.query_row(
        "SELECT (SELECT count(*) FROM journal_entry
                  WHERE origin_id = ?1 AND origin_kind IN ('Amortizacion', 'BajaInmovilizado')),
                EXISTS(SELECT 1 FROM leasing WHERE asset_id = ?1),
                prior_depreciation > 0 OR prior_until IS NOT NULL OR opening_id IS NOT NULL
           FROM fixed_asset WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if asientos > 0 {
        return Err(other(format!(
            "«{}» tiene asientos (amortizaciones o la baja): deshazlos antes de borrarlo",
            a.name
        )));
    }
    if leasing {
        return Err(other(format!(
            "«{}» es el bien de un arrendamiento financiero: se quita desde Leasing",
            a.name
        )));
    }
    if prior {
        return Err(other(format!(
            "«{}» viene de los saldos iniciales: se quita desde Saldos iniciales",
            a.name
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM asset_tax_incentive WHERE asset_id = ?1", [id])?;
    tx.execute("DELETE FROM fixed_asset WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.delete",
        &id.to_string(),
        &serde_json::json!({ "asset": a }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------- Deshacer una baja ----------------

/// Lo que ha hecho deshacer una baja.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BajaDeshecha {
    /// Asiento de la baja borrado.
    pub disposal_entry: i64,
    /// Asientos de amortización que vuelven (rehechos o registrados de nuevo).
    pub restored: Vec<i64>,
    /// Asiento de amortización que había creado la baja y se ha quitado.
    pub removed: Option<i64>,
    pub warnings: Vec<String>,
}

/// La auditoría de la última baja del elemento (lo que se rehízo y quitó al darla).
fn last_disposal_audit(conn: &Connection, id: i64) -> Result<Option<serde_json::Value>> {
    let details: Option<String> = conn
        .query_row(
            "SELECT details FROM audit_log WHERE action = 'asset.dispose' AND subject = ?1
              ORDER BY seq DESC LIMIT 1",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(details.and_then(|d| serde_json::from_str(&d).ok()))
}

/// Deshace la baja o la venta del elemento `id` (ver el módulo). Pide confirmación.
pub fn undo_disposal(
    v: &Vault,
    id: i64,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<BajaDeshecha> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let mut a = asset(v, id)?;
    let Some(date) = a.disposed_on else {
        return Err(other("ese elemento no está dado de baja"));
    };
    let conn = v.connection();
    let sold_with: Option<String> = conn
        .query_row(
            "SELECT i.number FROM asset_sale_invoice s JOIN invoice_issued i ON i.id = s.invoice_id
              WHERE s.asset_id = ?1 LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(n) = sold_with {
        return Err(other(format!(
            "la venta de «{}» tiene vinculada la factura {n}: desvincúlala antes",
            a.name
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let fy = fiscal_year_for_date(&tx, date)?;
    if year_closed(&tx, fy.year)? {
        return Err(other(format!(
            "la baja es del ejercicio {}, que está cerrado: reábrelo desde Cierre para deshacerla",
            fy.label()
        )));
    }
    let disposal_entry: Option<i64> = tx.query_row(
        "SELECT disposal_entry FROM fixed_asset WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let disposal_entry = match disposal_entry {
        Some(e) => e,
        None => tx
            .query_row(
                "SELECT id FROM journal_entry WHERE origin_kind = 'BajaInmovilizado'
                   AND origin_id = ?1 ORDER BY id LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| other("no se encuentra el asiento de la baja"))?,
    };
    for (table, column, why) in crate::import::ENTRY_REFERENCES {
        if table == "fixed_asset" {
            continue;
        }
        let used: bool = tx.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
            [disposal_entry],
            |r| r.get(0),
        )?;
        if used {
            return Err(other(format!(
                "el asiento de la baja {why}: deshaz eso primero"
            )));
        }
    }
    let audit_baja = last_disposal_audit(&tx, id)?;
    let mut warnings = Vec::new();
    let mut restored = Vec::new();
    let mut removed = None;
    a.disposed_on = None;
    // Asiento al que pasan los documentos de la baja (si hay amortización del ejercicio).
    let mut target: Option<i64> = None;
    let year_entry: Option<(i64, String)> = tx
        .query_row(
            "SELECT id, date FROM journal_entry WHERE origin_kind = 'Amortizacion'
               AND origin_id = ?1 AND fiscal_year = ?2",
            params![id, fy.year],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let rewritten = audit_baja
        .as_ref()
        .and_then(|d| d.get("depreciation"))
        .filter(|d| !d.is_null());
    let previous: Option<JournalEntry> = rewritten
        .and_then(|r| r.get("before"))
        .and_then(|b| serde_json::from_value(b.clone()).ok());
    match (year_entry, previous) {
        // La baja rehízo hasta su fecha la amortización ya registrada: vuelve a como estaba.
        (Some((e, _)), Some(old)) => {
            let cuota: Money = old.lines.iter().map(|l| l.debit).sum();
            tx.execute(
                "UPDATE journal_entry SET date = ?1 WHERE id = ?2",
                params![old.date.to_string(), e],
            )?;
            tx.execute(
                "UPDATE journal_line SET debit = ?1 WHERE entry_id = ?2 AND debit > 0",
                params![cuota.cents(), e],
            )?;
            tx.execute(
                "UPDATE journal_line SET credit = ?1 WHERE entry_id = ?2 AND credit > 0",
                params![cuota.cents(), e],
            )?;
            restored.push(e);
            target = Some(e);
        }
        // La baja la quitó (no quedaba cuota hasta ese día): se vuelve a registrar.
        (None, Some(old)) => {
            let e = record_entry(&tx, &old, now)?;
            restored.push(e);
            target = Some(e);
        }
        // Sin amortización registrada antes de la baja: la que hizo la baja (hasta su día) se
        // quita, y la del ejercicio se registra luego con las demás.
        (Some((e, _)), None) if rewritten.is_none() && audit_baja.is_some() => {
            if with_attachments(&tx, e)? {
                return Err(other(
                    "la amortización que registró la baja tiene documentos u observaciones en el \
                     Diario: quítalos antes de deshacerla",
                ));
            }
            delete_entry_rows(&tx, e)?;
            removed = Some(e);
            warnings.push(format!(
                "Se ha quitado la amortización de {} que registró la baja: registra la del \
                 ejercicio con las demás.",
                fy.label()
            ));
        }
        // Sin auditoría de la baja (de una versión antigua): la del ejercicio se rehace entera.
        (Some((e, d)), None) => {
            if parse_date(&d)? != fy.end {
                let (period, before) = crate::apertura::depreciation_basis(&tx, &a, &fy)?;
                if let Some(new) = period.and_then(|p| depreciation_entry_in(&a, &p, before)) {
                    let cuota: Money = new.lines.iter().map(|l| l.debit).sum();
                    tx.execute(
                        "UPDATE journal_entry SET date = ?1 WHERE id = ?2",
                        params![new.date.to_string(), e],
                    )?;
                    tx.execute(
                        "UPDATE journal_line SET debit = ?1 WHERE entry_id = ?2 AND debit > 0",
                        params![cuota.cents(), e],
                    )?;
                    tx.execute(
                        "UPDATE journal_line SET credit = ?1 WHERE entry_id = ?2 AND credit > 0",
                        params![cuota.cents(), e],
                    )?;
                }
            }
            restored.push(e);
            target = Some(e);
        }
        (None, None) => {}
    }
    // Las de ejercicios posteriores que quitó la baja.
    let later = audit_baja
        .as_ref()
        .and_then(|d| d.get("later_removed"))
        .and_then(|l| l.as_array().cloned())
        .unwrap_or_default();
    for l in &later {
        let Some(old) = l
            .get("deleted")
            .and_then(|d| serde_json::from_value::<JournalEntry>(d.clone()).ok())
        else {
            continue;
        };
        let y = fiscal_year_for_date(&tx, old.date)?;
        if year_closed(&tx, y.year)? {
            return Err(other(format!(
                "la baja quitó la amortización de {}, que ahora está cerrado: reábrelo para \
                 deshacer la baja",
                y.label()
            )));
        }
        restored.push(record_entry(&tx, &old, now)?);
    }
    if !later.is_empty() {
        warnings.push(
            "Vuelve la amortización de los ejercicios posteriores que había quitado la baja."
                .into(),
        );
    }
    if audit_baja.is_none() {
        warnings.push(
            "Esta baja es de una versión anterior: registra de nuevo las amortizaciones de los \
             ejercicios siguientes que falten."
                .into(),
        );
    }
    // Los documentos y observaciones del asiento de la baja no se pierden.
    let moved = match target {
        Some(t) => move_attachments(&tx, disposal_entry, t)?,
        None => {
            if with_attachments(&tx, disposal_entry)? {
                return Err(other(
                    "el asiento de la baja tiene documentos u observaciones en el Diario: \
                     quítalos antes de deshacerla",
                ));
            }
            serde_json::Value::Null
        }
    };
    let deleted = load_entry(&tx, disposal_entry)?;
    tx.execute(
        "UPDATE fixed_asset SET disposed_on = NULL, sale_price = NULL, disposal_entry = NULL
          WHERE id = ?1",
        [id],
    )?;
    delete_entry_rows(&tx, disposal_entry)?;
    if deleted.lines.iter().any(|l| l.account.as_str() == "543") {
        warnings.push(
            "Si cobraste la venta contra la 543, deshaz también esa conciliación en Banco.".into(),
        );
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.undispose",
        &id.to_string(),
        &serde_json::json!({
            "date": date.to_string(),
            "deleted": deleted,
            "restored": restored,
            "removed": removed,
            "moved": moved,
        }),
    )?;
    tx.commit()?;
    Ok(BajaDeshecha {
        disposal_entry,
        restored,
        removed,
        warnings,
    })
}

// ---------------- Amortizaciones del ejercicio ----------------

/// La amortización de un elemento en un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineaAmortizacion {
    pub asset_id: i64,
    pub name: String,
    pub amount: Money,
    /// El asiento, si ya está registrada.
    pub entry_id: Option<i64>,
}

/// Lo que registraría «Registrar amortizaciones» y lo que ya está registrado del ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrevisionAmortizacion {
    pub year: i32,
    pub pendientes: Vec<LineaAmortizacion>,
    pub total: Money,
    pub registradas: Vec<LineaAmortizacion>,
    pub total_registrado: Money,
    pub cerrado: bool,
}

/// La previsión de las amortizaciones del ejercicio `year` (las mismas cuotas que registra
/// [`crate::accounting::post_depreciation`]).
pub fn depreciation_preview(v: &Vault, year: i32) -> Result<PrevisionAmortizacion> {
    let assets = fixed_assets(v)?;
    let conn = v.connection();
    let fy = fiscal_year_conn(conn, year)?;
    let mut pendientes = Vec::new();
    let mut registradas = Vec::new();
    for a in &assets {
        let done: Option<(i64, i64)> = conn
            .query_row(
                "SELECT e.id, COALESCE((SELECT SUM(l.debit) FROM journal_line l
                                         WHERE l.entry_id = e.id), 0)
                   FROM journal_entry e WHERE e.origin_kind = 'Amortizacion'
                    AND e.origin_id = ?1 AND e.fiscal_year = ?2",
                params![a.id, year],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((e, c)) = done {
            registradas.push(LineaAmortizacion {
                asset_id: a.id,
                name: a.name.clone(),
                amount: Money::from_cents(c),
                entry_id: Some(e),
            });
            continue;
        }
        if let (Some(period), before) = crate::apertura::depreciation_basis(conn, a, &fy)?
            && let Some(e) = depreciation_entry_in(a, &period, before)
        {
            pendientes.push(LineaAmortizacion {
                asset_id: a.id,
                name: a.name.clone(),
                amount: e.lines.iter().map(|l| l.debit).sum(),
                entry_id: None,
            });
        }
    }
    Ok(PrevisionAmortizacion {
        year,
        total: pendientes.iter().map(|l| l.amount).sum(),
        total_registrado: registradas.iter().map(|l| l.amount).sum(),
        pendientes,
        registradas,
        cerrado: year_closed(conn, year)?,
    })
}

/// Lo que ha hecho deshacer amortizaciones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AmortizacionDeshecha {
    pub borrados: usize,
    pub total: Money,
    /// Las que no se han podido deshacer y por qué.
    pub saltados: Vec<String>,
}

/// Deshace la amortización registrada del ejercicio abierto `year` (de todos los elementos o
/// sólo de `asset`): borra sus asientos. No se deshace la de un elemento dado de baja en ese
/// ejercicio o antes (forma parte de la baja), la que no es la última del elemento ni la que
/// tiene documentos u observaciones. Pide confirmación.
pub fn undo_depreciation(
    v: &Vault,
    year: i32,
    asset_id: Option<i64>,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<AmortizacionDeshecha> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let assets = fixed_assets(v)?;
    let conn = v.connection();
    let fy = fiscal_year_conn(conn, year)?;
    if year_closed(conn, year)? {
        return Err(other(format!(
            "el ejercicio {} está cerrado: reábrelo desde Cierre para deshacer su amortización",
            fy.label()
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let mut out = AmortizacionDeshecha {
        borrados: 0,
        total: Money::ZERO,
        saltados: Vec::new(),
    };
    let mut deleted = Vec::new();
    for a in assets.iter().filter(|a| asset_id.is_none_or(|x| x == a.id)) {
        let hechas = posted(&tx, a.id)?;
        let Some(&(e, _, _, cuota)) = hechas.iter().find(|h| h.1 == year) else {
            continue;
        };
        if let Some(d) = a.disposed_on
            && d <= fy.end
        {
            out.saltados.push(format!(
                "«{}»: forma parte de su baja del {}; deshaz antes la baja",
                a.name,
                dmy(d)
            ));
            continue;
        }
        if let Some(l) = hechas.iter().find(|h| h.1 > year) {
            out.saltados.push(format!(
                "«{}»: tiene registrada la de {}; deshaz antes esa",
                a.name, l.1
            ));
            continue;
        }
        if with_attachments(&tx, e)? {
            out.saltados.push(format!(
                "«{}»: su asiento tiene documentos u observaciones en el Diario; quítalos antes",
                a.name
            ));
            continue;
        }
        deleted.push(serde_json::json!({ "asset": a.id, "entry": load_entry(&tx, e)? }));
        delete_entry_rows(&tx, e)?;
        out.borrados += 1;
        out.total += cuota;
    }
    if out.borrados > 0 {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "depreciation.undo",
            &year.to_string(),
            &serde_json::json!({ "asset": asset_id, "deleted": deleted }),
        )?;
    }
    tx.commit()?;
    if out.borrados == 0 && out.saltados.is_empty() {
        return Err(other(format!(
            "no hay amortización registrada de {} que deshacer",
            fy.label()
        )));
    }
    Ok(out)
}

// ---------------- Incentivos fiscales ----------------

/// Incentivo fiscal con el que se amortiza un elemento.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Incentivo {
    /// Elementos nuevos de valor unitario hasta 300 €, hasta 25.000 € al año (art. 12.3.e LIS).
    EscasoValor,
    /// Libertad de amortización con creación de empleo (art. 102 LIS). `incremento_plantilla`:
    /// aumento de la plantilla media, en centésimas de persona (1,50 personas = 150).
    ErdEmpleo { incremento_plantilla: i64 },
    /// Amortización acelerada (art. 103 LIS). `coef_tablas`: coeficiente lineal máximo de las
    /// tablas del elemento; el fiscal puede llegar al doble.
    ErdAcelerada { coef_tablas: Rate },
    /// Otra libertad de amortización, sin topes que mirar aquí.
    Otro,
}

impl Incentivo {
    fn kind(&self) -> &'static str {
        match self {
            Incentivo::EscasoValor => "escaso_valor",
            Incentivo::ErdEmpleo { .. } => "erd_empleo",
            Incentivo::ErdAcelerada { .. } => "erd_acelerada",
            Incentivo::Otro => "otro",
        }
    }

    fn param(&self) -> Option<i64> {
        match self {
            Incentivo::ErdEmpleo {
                incremento_plantilla,
            } => Some(*incremento_plantilla),
            Incentivo::ErdAcelerada { coef_tablas } => Some(i64::from(coef_tablas.bp())),
            Incentivo::EscasoValor | Incentivo::Otro => None,
        }
    }

    fn from_row(kind: &str, param: Option<i64>) -> Option<Incentivo> {
        Some(match kind {
            "escaso_valor" => Incentivo::EscasoValor,
            "erd_empleo" => Incentivo::ErdEmpleo {
                incremento_plantilla: param.unwrap_or(0),
            },
            "erd_acelerada" => Incentivo::ErdAcelerada {
                coef_tablas: Rate::from_bp(i32::try_from(param.unwrap_or(0)).unwrap_or(0)),
            },
            "otro" => Incentivo::Otro,
            _ => return None,
        })
    }

    /// ¿Es de la empresa de reducida dimensión?
    fn de_erd(&self) -> bool {
        matches!(
            self,
            Incentivo::ErdEmpleo { .. } | Incentivo::ErdAcelerada { .. }
        )
    }
}

/// El incentivo de un elemento, si tiene.
pub(crate) fn incentive_of(conn: &Connection, id: i64) -> Result<Option<Incentivo>> {
    let row: Option<(String, Option<i64>)> = conn
        .query_row(
            "SELECT kind, param FROM asset_tax_incentive WHERE asset_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(k, p)| Incentivo::from_row(&k, p)))
}

/// El coeficiente fiscal que pide el incentivo (con el `pedido`, si lo admite) o el error.
fn tax_rate_for(i: &Incentivo, pedido: Option<Rate>) -> Result<Rate> {
    let cien = Rate::from_bp(10_000);
    match i {
        Incentivo::EscasoValor | Incentivo::ErdEmpleo { .. } => match pedido {
            Some(r) if r != cien => Err(other(
                "la libertad de amortización es del 100 %: deja el coeficiente fiscal vacío o \
                 en el 100 %",
            )),
            _ => Ok(cien),
        },
        Incentivo::ErdAcelerada { coef_tablas } => {
            if coef_tablas.bp() <= 0 || coef_tablas.bp() > 10_000 {
                return Err(other(
                    "pon el coeficiente lineal máximo de las tablas del elemento (entre 0 y 100 %)",
                ));
            }
            let tope = Rate::from_bp((coef_tablas.bp() * 2).min(10_000));
            match pedido {
                Some(r) if r.bp() <= 0 || r > tope => Err(other(format!(
                    "con la amortización acelerada el coeficiente fiscal llega como mucho al \
                     doble del de tablas: {tope} (art. 103.1 LIS)"
                ))),
                Some(r) => Ok(r),
                None => Ok(tope),
            }
        }
        Incentivo::Otro => match pedido {
            Some(r) if r.bp() > 0 && r.bp() <= 10_000 => Ok(r),
            Some(_) => Err(other("el coeficiente fiscal debe estar entre 0 y 100 %")),
            None => Ok(cien),
        },
    }
}

/// Comprueba que el coeficiente fiscal `tax_rate` vale con el incentivo del elemento (para
/// cambiar el plan de amortización sin saltarse sus topes).
pub(crate) fn check_plan(conn: &Connection, id: i64, tax_rate: Option<Rate>) -> Result<()> {
    let Some(i) = incentive_of(conn, id)? else {
        return Ok(());
    };
    match tax_rate {
        None => Err(other(
            "el elemento tiene un incentivo fiscal: quítalo antes de dejarlo sin coeficiente \
             fiscal propio",
        )),
        Some(r) => tax_rate_for(&i, Some(r)).map(|_| ()),
    }
}

/// Uso de los topes de los incentivos en un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UsoIncentivos {
    pub year: i32,
    /// Coste de los elementos de escaso valor dados de alta en el ejercicio.
    pub escaso_valor: Money,
    pub escaso_valor_tope: Money,
    /// Base amortizable de los elementos con libertad de amortización con creación de empleo.
    pub erd_empleo: Money,
    /// 120.000 € por el incremento de plantilla (el mayor de los indicados en el ejercicio).
    pub erd_empleo_tope: Option<Money>,
}

/// Un elemento con su incentivo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IncentivoBien {
    pub asset_id: i64,
    pub incentivo: Incentivo,
}

/// Los incentivos de los elementos.
pub fn incentives(v: &Vault) -> Result<Vec<IncentivoBien>> {
    let conn = v.connection();
    let mut st =
        conn.prepare("SELECT asset_id, kind, param FROM asset_tax_incentive ORDER BY asset_id")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (asset_id, k, p) = r?;
        if let Some(incentivo) = Incentivo::from_row(&k, p) {
            out.push(IncentivoBien {
                asset_id,
                incentivo,
            });
        }
    }
    Ok(out)
}

/// Cuánto se ha usado de cada tope en el ejercicio `year` (por la fecha de alta de los
/// elementos), sin contar el elemento `except`.
fn uso(conn: &Connection, year: i32, except: Option<i64>) -> Result<UsoIncentivos> {
    let fy = fiscal_year_conn(conn, year)?;
    let mut st = conn.prepare(
        "SELECT a.id, a.cost, a.residual, i.kind, i.param FROM fixed_asset a
           JOIN asset_tax_incentive i ON i.asset_id = a.id
          WHERE a.acquired_on BETWEEN ?1 AND ?2",
    )?;
    let rows = st.query_map(params![fy.start.to_string(), fy.end.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<i64>>(4)?,
        ))
    })?;
    let mut u = UsoIncentivos {
        year,
        escaso_valor: Money::ZERO,
        escaso_valor_tope: ESCASO_VALOR_ANUAL,
        erd_empleo: Money::ZERO,
        erd_empleo_tope: None,
    };
    for r in rows {
        let (id, cost, residual, kind, param) = r?;
        if Some(id) == except {
            continue;
        }
        match kind.as_str() {
            "escaso_valor" => u.escaso_valor += Money::from_cents(cost),
            "erd_empleo" => {
                u.erd_empleo += Money::from_cents(cost - residual);
                let tope = tope_empleo(param.unwrap_or(0));
                u.erd_empleo_tope = Some(u.erd_empleo_tope.map_or(tope, |t| t.max(tope)));
            }
            _ => {}
        }
    }
    Ok(u)
}

/// 120.000 € por el incremento de plantilla en centésimas.
fn tope_empleo(centesimas: i64) -> Money {
    Money::from_cents(ERD_EMPLEO_POR_PERSONA.cents() * centesimas / 100)
}

/// Uso de los topes de los incentivos en el ejercicio `year`.
pub fn incentive_usage(v: &Vault, year: i32) -> Result<UsoIncentivos> {
    uso(v.connection(), year, None)
}

/// Pone (o quita, con `None`) el incentivo fiscal de un elemento y, con él, su coeficiente
/// fiscal: el 100 % en la libertad de amortización, hasta el doble del de tablas en la
/// acelerada (`tax_rate` o, si no se da, el doble). Comprueba los topes; lo que no se puede
/// comprobar (empresa de reducida dimensión, elementos nuevos, mantenimiento del empleo) va en
/// los avisos. Sin incentivo, el coeficiente fiscal queda en `tax_rate`.
pub fn set_incentive(
    v: &Vault,
    id: i64,
    incentivo: Option<Incentivo>,
    tax_rate: Option<Rate>,
    now: OffsetDateTime,
) -> Result<Vec<String>> {
    let a = asset(v, id)?;
    if a.disposed_on.is_some() {
        return Err(other("el elemento está dado de baja"));
    }
    let conn = v.connection();
    let year = fiscal_year_for_date(conn, a.acquired_on)?.year;
    let mut avisos = Vec::new();
    let rate = match &incentivo {
        None => tax_rate,
        Some(i) => Some(tax_rate_for(i, tax_rate)?),
    };
    let grupo = |p: &[&str]| p.iter().any(|x| a.account.starts_with(x));
    match &incentivo {
        Some(Incentivo::EscasoValor) => {
            if !grupo(&["21"]) {
                return Err(other(
                    "los elementos de escaso valor son del inmovilizado material (21x)",
                ));
            }
            if a.cost > ESCASO_VALOR_UNITARIO {
                return Err(other(format!(
                    "«{}» cuesta {}: los elementos de escaso valor no pasan de {} por unidad \
                     (art. 12.3.e LIS)",
                    a.name, a.cost, ESCASO_VALOR_UNITARIO
                )));
            }
            let u = uso(conn, year, Some(id))?;
            if u.escaso_valor + a.cost > ESCASO_VALOR_ANUAL {
                return Err(other(format!(
                    "pasaría del límite de {} al año de los elementos de escaso valor (ya hay {} \
                     en {year}; art. 12.3.e LIS)",
                    ESCASO_VALOR_ANUAL, u.escaso_valor
                )));
            }
            avisos.push(
                "Sólo para elementos nuevos (art. 12.3.e LIS); un elemento de segunda mano se \
                 amortiza por tablas."
                    .into(),
            );
        }
        Some(Incentivo::ErdEmpleo {
            incremento_plantilla,
        }) => {
            if !grupo(&["21", "22"]) {
                return Err(other(
                    "la libertad de amortización con creación de empleo es del inmovilizado \
                     material y las inversiones inmobiliarias (21x y 22x; art. 102.1 LIS)",
                ));
            }
            if *incremento_plantilla <= 0 {
                return Err(other(
                    "pon el incremento de la plantilla media (con dos decimales): sin aumento de \
                     plantilla no hay libertad de amortización (art. 102.1 LIS)",
                ));
            }
            let tope = tope_empleo(*incremento_plantilla);
            let u = uso(conn, year, Some(id))?;
            let base = a.cost - a.residual;
            if u.erd_empleo + base > tope {
                return Err(other(format!(
                    "pasaría de la inversión máxima de {tope} (120.000 € por un incremento de \
                     plantilla de {}; ya hay {} en {year}; art. 102.2 LIS)",
                    fmt_centesimas(*incremento_plantilla),
                    u.erd_empleo
                )));
            }
            avisos.push(
                "Requisito de empleo: la plantilla media total de los 24 meses siguientes al \
                 inicio del ejercicio en que el elemento entra en funcionamiento tiene que \
                 superar la de los 12 meses anteriores, y mantenerse otros 24 meses. Si no se \
                 cumple, hay que ingresar la cuota de lo amortizado de más con sus intereses de \
                 demora (art. 102 LIS). TaxPain no lo vigila: apúntatelo."
                    .into(),
            );
        }
        Some(Incentivo::ErdAcelerada { .. }) => {
            if a.account.starts_with("20") {
                avisos.push(
                    "Intangible: el intangible de vida útil indefinida y el fondo de comercio \
                     se amortizan fiscalmente al 150 % de lo del art. 12.2 LIS, no al doble \
                     (art. 103.1 LIS)."
                        .into(),
                );
            }
        }
        Some(Incentivo::Otro) => avisos.push(
            "Sin topes que comprobar: revisa los requisitos del precepto que aplicas.".into(),
        ),
        None => {}
    }
    if let Some(i) = &incentivo {
        if i.de_erd() {
            avisos.push(erd_aviso(conn, year)?);
        }
        if i.de_erd() || matches!(i, Incentivo::EscasoValor) {
            avisos.push("Sólo para elementos nuevos: los usados no tienen este incentivo.".into());
        }
    }
    avisos.dedup();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE fixed_asset SET tax_rate_bp = ?1 WHERE id = ?2",
        params![rate.map(Rate::bp), id],
    )?;
    match &incentivo {
        Some(i) => {
            tx.execute(
                "INSERT INTO asset_tax_incentive(asset_id, kind, param, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(asset_id) DO UPDATE SET kind = excluded.kind, param = excluded.param",
                params![id, i.kind(), i.param(), now_str(now)],
            )?;
        }
        None => {
            tx.execute("DELETE FROM asset_tax_incentive WHERE asset_id = ?1", [id])?;
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.incentive",
        &id.to_string(),
        &serde_json::json!({ "incentive": incentivo, "tax_rate": rate.map(Rate::bp) }),
    )?;
    tx.commit()?;
    avisos.push(
        "La diferencia con la amortización contable va como ajuste al modelo 200 (diferencias de \
         amortización) y se revierte en los años siguientes y en la baja."
            .into(),
    );
    Ok(avisos)
}

fn fmt_centesimas(c: i64) -> String {
    format!("{},{:02}", c / 100, (c % 100).abs())
}

/// Aviso de la condición de empresa de reducida dimensión con la cifra de negocios del ejercicio
/// anterior que da el diario (ventas, 70x).
fn erd_aviso(conn: &Connection, year: i32) -> Result<String> {
    let ventas: Option<i64> = conn.query_row(
        "SELECT SUM(l.credit - l.debit) FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND l.account LIKE '70%'
            AND e.origin_kind NOT IN ('Regularizacion', 'Cierre')",
        [year - 1],
        |r| r.get(0),
    )?;
    Ok(match ventas.map(Money::from_cents) {
        Some(v) if v >= DIEZ_MILLONES => format!(
            "La cifra de negocios de {} según el diario es {v}: con 10 M€ o más no eres empresa \
             de reducida dimensión, salvo la prórroga de los tres años siguientes (art. 101.4 \
             LIS). Compruébalo antes de aplicar el incentivo.",
            year - 1
        ),
        Some(v) => format!(
            "Empresa de reducida dimensión: la cifra de negocios de {} según el diario es {v}, \
             menos de 10 M€ (art. 101 LIS; si formas grupo, cuenta la del grupo).",
            year - 1
        ),
        None => format!(
            "Sólo para empresas de reducida dimensión: cifra de negocios de {} inferior a 10 M€ \
             (art. 101 LIS). No hay ventas de ese año en el diario para comprobarlo.",
            year - 1
        ),
    })
}
