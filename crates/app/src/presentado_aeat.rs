//! «Comprobar lo presentado en el año»: qué declaraciones tiene la AEAT de la empresa (también
//! las que presentó la gestoría), cuáles tocaban según el perfil fiscal y faltan, los
//! justificantes en PDF y la comparación casilla a casilla con lo que calcula TaxPain.
//!
//! La red va en la capa de la aplicación de escritorio (consulta de declaraciones presentadas
//! con el certificado y cotejo por CSV); aquí se guarda lo que devuelve la AEAT y se arma el
//! resultado:
//! 1. `models_to_query`: los modelos que se consultan (los del perfil y los habituales).
//! 2. `record`: guarda la lista de la AEAT (una fila por justificante, con su CSV).
//! 3. `pending` + `attach`: los justificantes que faltan por descargar, las casillas que faltan
//!    por leer y lo descargado (el PDF y el fichero de la declaración, del que salen las
//!    casillas exactas).
//! 4. `report`: la tabla por modelo y periodo.
//!
//! «Falta» sólo se dice cuando la obligación es segura (303, 390, 130/131, 200 de sociedades, 100
//! de personas físicas, 202 de grandes empresas) o cuando TaxPain calcula importes en ese periodo.
//! Los modelos que sólo tocan si hubo retenciones, pagos u operaciones salen como «No consta en
//! la AEAT», con una nota de cuándo hacía falta.
//!
//! El CSV da acceso al documento en la Sede: se guarda sólo en el almacén cifrado y no sale en
//! el registro de auditoría ni en el resultado.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::calendar::{Model, Persona, TaxpayerProfile, calendar_for, deadlines_in_year};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::presentados;

/// Modelos que se consultan siempre (los que presentan las sociedades y los autónomos; el
/// servicio de la AEAT consulta modelo a modelo).
pub const COMMON: [&str; 20] = [
    "303", "390", "111", "190", "115", "180", "123", "193", "130", "131", "349", "347", "200",
    "202", "232", "216", "296", "369", "720", "721",
];

/// Declaraciones informativas. ConsultaExt sí lista las informativas (comprobado con
/// declaraciones reales). El 190, el 180
/// y el 193 faltan cuando hubo retenciones ese año en sus trimestrales (111, 115 y 123); de las
/// demás no se sabe si tocaban. `TODO(VERIFY)`: no todas se han comprobado con declaraciones
/// reales.
pub const INFORMATIVE: [&str; 12] = [
    "180", "182", "184", "190", "193", "232", "270", "296", "347", "349", "720", "721",
];

/// Última comprobación de un año (en ajustes).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Check {
    nif: String,
    checked_at: String,
    models: Vec<String>,
    errors: Vec<String>,
    /// Lo que contestó la AEAT para los modelos que se dieron por «sin declaraciones» (modelo →
    /// texto). Se guarda para que el usuario lo vea: el texto exacto no está documentado.
    #[serde(default)]
    empty: BTreeMap<String, String>,
    /// Modelos que el servicio de la AEAT no permite consultar, con su texto (el 721): no se sabe
    /// si se presentaron.
    #[serde(default)]
    unavailable: BTreeMap<String, String>,
}

const CHECKS_KEY: &str = "aeat_filing_checks";

fn checks(v: &Vault) -> Result<BTreeMap<i32, Check>> {
    Ok(v.setting(CHECKS_KEY)?.unwrap_or_default())
}

/// NIF de la empresa abierta (el de la facturación o el de la ficha de la empresa).
pub fn company_nif(v: &Vault) -> Option<String> {
    crate::carpeta::own_nif(v)
}

/// Nombre o razón social de la empresa abierta (para la consulta de la AEAT).
pub fn company_name(v: &Vault) -> Option<String> {
    crate::companies::info(v)
        .ok()
        .map(|i| i.name.trim().to_owned())
        .filter(|n| !n.is_empty())
}

fn profile(v: &Vault) -> Result<Option<TaxpayerProfile>> {
    Ok(v.setting::<TaxpayerProfile>("taxpayer_profile")?)
}

/// Modelos y periodos que tocaban en el ejercicio `year` según el perfil, con su último día.
pub fn expected(p: &TaxpayerProfile, year: i32) -> BTreeMap<(String, String), Date> {
    let mut out = BTreeMap::new();
    for año in [year, year + 1, year + 2] {
        let (h, _) = calendar_for(año, p.ccaa, &p.festivos_locales);
        for d in deadlines_in_year(p, año, &h) {
            let code = d.model.code();
            // El segundo plazo de la Renta es un pago, no otra declaración.
            if d.fiscal_year != year
                || d.model == Model::M100SegundoPlazo
                || !code.bytes().all(|b| b.is_ascii_digit())
            {
                continue;
            }
            out.insert((code.to_owned(), d.period.code()), d.due_on);
        }
    }
    out
}

/// ¿Es un NIF de persona física? (DNI o NIE, y los K, L y M.)
fn natural_person_nif(nif: &str) -> bool {
    crate::perfil_deducido::nif_de_persona_fisica(nif)
}

/// Modelos que se consultan para el año: los habituales y los que pide el perfil.
pub fn models_to_query(v: &Vault, year: i32) -> Result<Vec<String>> {
    let mut set: BTreeSet<String> = COMMON.iter().map(|s| (*s).to_owned()).collect();
    match profile(v)? {
        Some(p) => {
            set.extend(expected(&p, year).into_keys().map(|(m, _)| m));
            if p.persona == Persona::Fisica {
                set.insert("100".into());
            }
        }
        // Sin perfil, la Renta se consulta si el NIF es de persona física.
        None => {
            if company_nif(v).is_some_and(|n| natural_person_nif(&n)) {
                set.insert("100".into());
            }
        }
    }
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort_by_key(|m| order(m));
    Ok(v)
}

/// Orden de los modelos en la tabla: los de la lista común primero, en su orden.
fn order(model: &str) -> (usize, String) {
    (
        COMMON
            .iter()
            .position(|m| *m == model)
            .unwrap_or(COMMON.len()),
        model.to_owned(),
    )
}

/// Una declaración según la AEAT (lo que devuelve la consulta de declaraciones presentadas).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presented {
    pub model: String,
    pub year: i32,
    pub period: String,
    pub csv: String,
    pub file_number: String,
    pub receipt: String,
    pub previous_receipt: Option<String>,
    pub presented_at: String,
}

/// Lo que devolvió la AEAT al consultar un año.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// Modelos consultados.
    pub models: Vec<String>,
    /// Declaraciones encontradas.
    pub list: Vec<Presented>,
    /// Modelos que no se pudieron consultar («modelo: mensaje»).
    pub errors: Vec<String>,
    /// Modelos sin declaraciones, con el texto con que lo dijo la AEAT.
    pub empty: BTreeMap<String, String>,
    /// Modelos que el servicio no permite consultar, con el texto de la AEAT.
    pub unavailable: BTreeMap<String, String>,
}

/// Guarda lo que devolvió la AEAT para el año. Una fila por justificante; las que ya estaban se
/// actualizan. `nif` es el NIF consultado: si la empresa tiene NIF, tiene que ser el suyo.
pub fn record(v: &Vault, nif: &str, year: i32, a: &Answer, now: OffsetDateTime) -> Result<()> {
    let nif = nif.trim().to_uppercase();
    if let Some(own) = company_nif(v)
        && own != nif
    {
        return Err(AppError::Other(format!(
            "lo consultado es de {nif} y la empresa abierta es {own}: no se guarda"
        )));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    for p in &a.list {
        tx.execute(
            "INSERT INTO aeat_filing(nif, model, year, period, csv, file_number, receipt,
                                     previous_receipt, presented_at, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(csv) DO UPDATE SET previous_receipt = excluded.previous_receipt,
                                            checked_at = excluded.checked_at",
            params![
                nif,
                p.model,
                p.year,
                p.period.to_uppercase(),
                p.csv,
                p.file_number,
                p.receipt,
                p.previous_receipt.as_deref().filter(|s| !s.is_empty()),
                p.presented_at,
                now_str(now)
            ],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "aeat_filing.check",
        &year.to_string(),
        &serde_json::json!({
            "models": a.models.len(),
            "filings": a.list.len(),
            "errors": a.errors.len(),
            "empty": a.empty.len(),
            "unavailable": a.unavailable.len(),
        }),
    )?;
    tx.commit()?;
    let mut all = checks(v)?;
    all.insert(
        year,
        Check {
            nif,
            checked_at: now_str(now),
            models: a.models.clone(),
            errors: a.errors.clone(),
            empty: a.empty.clone(),
            unavailable: a.unavailable.clone(),
        },
    );
    v.set_setting(CHECKS_KEY, &all)?;
    Ok(())
}

/// Justificante pendiente: falta el PDF o, si ya está, las casillas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub csv: String,
    pub nif: String,
    pub model: String,
    pub period: String,
    /// El PDF ya está guardado: sólo faltan las casillas.
    pub has_pdf: bool,
    /// El visor de la declaración ya respondió (dio el fichero, que se guarda, o dijo que no lo
    /// tiene): no se le vuelve a pedir; las casillas, del fichero guardado o del PDF.
    pub visor_done: bool,
}

/// Declaraciones del año sin el PDF del justificante y las vigentes (no sustituidas por una
/// complementaria o sustitutiva) sin casillas, de cualquier modelo: se vuelven a intentar al
/// comprobar otra vez. No vuelve a pedir el PDF de lo que el cotejo dio por definitivo (no
/// encontrado, anulado, no cotejable).
/// También vuelven:
/// - las que el visor dio por «no disponible» desde 2019 y siguen sin casillas (el visor sólo no
///   tiene las de 2017 y principios de 2018: en otro año puede haber sido un fallo pasajero);
/// - las que tienen guardado un fichero de un diseño que TaxPain no conocía y ahora sí (tras
///   actualizar TaxPain): se releen del fichero, exacto, en lugar del PDF;
/// - las que tienen las casillas sacadas del PDF (aproximadas) sin que el visor haya contestado
///   de forma definitiva (no se le pudo preguntar, o dijo «no disponible» desde 2019): se le
///   vuelve a pedir el fichero, que da las casillas exactas.
pub fn pending(v: &Vault, year: i32) -> Result<Vec<Pending>> {
    let mut stmt = v.connection().prepare(
        "SELECT a.csv, a.nif, a.model, a.period, a.document_id IS NOT NULL,
                a.visor_state IS NOT NULL
                  AND NOT (a.visor_state = 'no_disponible' AND a.year >= 2019
                           AND (a.boxes IS NULL OR a.boxes_source = 'pdf')),
                IFNULL(a.visor_state, '') = 'diseno_desconocido' AND a.boxes IS NOT NULL,
                a.file, a.year
         FROM aeat_filing a
         WHERE a.year = ?1 AND (?2 IS NULL OR a.nif = ?2) AND a.download_state IS NULL
           AND (a.document_id IS NULL
                OR ((a.boxes IS NULL
                     OR (a.visor_state = 'diseno_desconocido' AND a.file IS NOT NULL)
                     OR (a.boxes_source = 'pdf'
                         AND (a.visor_state IS NULL
                              OR (a.visor_state = 'no_disponible' AND a.year >= 2019))))
                    AND NOT EXISTS (SELECT 1 FROM aeat_filing b
                                    WHERE b.previous_receipt = a.receipt
                                      AND b.model = a.model AND b.year = a.year)))
         ORDER BY a.model, a.period, a.presented_at",
    )?;
    let rows = stmt.query_map(params![year, company_nif(v)], |r| {
        let pending = Pending {
            csv: r.get(0)?,
            nif: r.get(1)?,
            model: r.get(2)?,
            period: r.get(3)?,
            has_pdf: r.get(4)?,
            visor_done: r.get(5)?,
        };
        let reread: bool = r.get(6)?;
        let file: Option<String> = r.get(7)?;
        let y: i32 = r.get(8)?;
        Ok((pending, reread, file, y))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (p, reread, file, y) = row?;
        // Ya leído del PDF con un fichero de diseño desconocido: sólo vuelve si ahora se conoce.
        if reread
            && file.is_none_or(|f| crate::lectura_justificante::unknown_design(&f, &p.model, y))
        {
            continue;
        }
        out.push(p);
    }
    Ok(out)
}

/// Resultado de la descarga del justificante de una declaración.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download<'a> {
    /// El PDF del justificante.
    Pdf(&'a [u8]),
    /// El PDF ya estaba guardado: sólo se intentan las casillas.
    Stored,
    /// No se pudo descargar ahora (se reintenta en la próxima comprobación).
    Failed(&'a str),
    /// Respuesta definitiva del cotejo (`state`: «no_encontrado», «anulado», «no_cotejable»):
    /// no se vuelve a pedir.
    Final { state: &'a str, message: &'a str },
}

/// Lo que dio el visor de la declaración de la Sede.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visor<'a> {
    /// El fichero de la declaración (se guarda).
    File(&'a str),
    /// Ya se pidió antes: se usa el fichero guardado, si lo hay.
    Stored,
    /// Respuesta definitiva: «no_disponible» (el visor no tiene el documento, como las
    /// declaraciones antiguas) o «sin_fichero». No se vuelve a pedir.
    Final(&'a str),
    /// No se ha pedido o falló la red: se intenta en la próxima comprobación.
    Unknown,
}

/// Guarda lo descargado de una declaración: el PDF del justificante y sus casillas. Las casillas
/// salen del fichero de la declaración (exacto) si es coherente; si no, del PDF (el recién
/// descargado o el ya guardado). Se guardan en la declaración (todos los modelos, para la
/// evolución y los cruces) y, de los que TaxPain compara, también como modelo presentado.
pub fn attach(
    v: &Vault,
    csv: &str,
    download: Download<'_>,
    visor: Visor<'_>,
    now: OffsetDateTime,
) -> Result<()> {
    use crate::lectura_justificante::{self as lj, Source};
    let conn = v.connection();
    #[allow(clippy::type_complexity)]
    let (id, model, year, period, receipt, stored, stored_file): (
        i64,
        String,
        i32,
        String,
        String,
        Option<Vec<u8>>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT a.id, a.model, a.year, a.period, a.receipt, d.content, a.file
             FROM aeat_filing a
             LEFT JOIN document d ON d.id = a.document_id WHERE a.csv = ?1",
            [csv],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let base = format!("{model}_{year}_{period}_{receipt}");
    let pdf: Option<&[u8]> = match download {
        Download::Pdf(pdf) => {
            if !pdf.starts_with(b"%PDF") {
                return Err(AppError::Other(
                    "el justificante descargado no es un PDF".into(),
                ));
            }
            let sha = hex::encode(Sha256::digest(pdf));
            let existing: Option<i64> = conn
                .query_row("SELECT id FROM document WHERE sha256 = ?1", [&sha], |r| {
                    r.get(0)
                })
                .optional()?;
            let doc = match existing {
                Some(d) => d,
                None => crate::invoicing::store_document(
                    conn,
                    "justificante_aeat",
                    &format!("{base}.pdf"),
                    "application/pdf",
                    pdf,
                    now,
                )?,
            };
            conn.execute(
                "UPDATE aeat_filing SET document_id = ?1, download_error = NULL,
                                        download_state = NULL
                 WHERE id = ?2",
                params![doc, id],
            )?;
            Some(pdf)
        }
        Download::Stored => stored.as_deref(),
        Download::Failed(e) => {
            conn.execute(
                "UPDATE aeat_filing SET download_error = ?1 WHERE id = ?2",
                params![e, id],
            )?;
            stored.as_deref()
        }
        Download::Final { state, message } => {
            conn.execute(
                "UPDATE aeat_filing SET download_error = ?1, download_state = ?2 WHERE id = ?3",
                params![message, state, id],
            )?;
            None
        }
    };
    match visor {
        Visor::Final(state) => {
            conn.execute(
                "UPDATE aeat_filing SET visor_state = ?1 WHERE id = ?2",
                params![state, id],
            )?;
        }
        Visor::File(f) => {
            let state = if lj::unknown_design(f, &model, year) {
                "diseno_desconocido"
            } else {
                "leido"
            };
            conn.execute(
                "UPDATE aeat_filing SET visor_state = ?1, file = ?2 WHERE id = ?3",
                params![state, f, id],
            )?;
        }
        // El fichero guardado de un diseño que antes no se conocía: si ya se conoce, se lee.
        Visor::Stored => {
            if let Some(f) = stored_file.as_deref()
                && !lj::unknown_design(f, &model, year)
            {
                conn.execute(
                    "UPDATE aeat_filing SET visor_state = 'leido'
                     WHERE id = ?1 AND visor_state = 'diseno_desconocido'",
                    [id],
                )?;
            }
        }
        Visor::Unknown => {}
    }
    let file = match visor {
        Visor::File(f) => Some(f),
        _ => stored_file.as_deref(),
    };
    // Casillas: el fichero si es coherente; si no, el PDF.
    let from_file = match file.map(Visor::File).unwrap_or(Visor::Unknown) {
        Visor::File(f) if model == "200" => {
            lj::from_file_200(f, year).map(|b| (b, Source::File, f.as_bytes()))
        }
        Visor::File(f) => {
            lj::from_file(f, &model, year, &period).map(|(b, s)| (b, s, f.as_bytes()))
        }
        _ => None,
    }
    .filter(|(b, _, _)| lj::plausible(&model, b));
    let read = from_file.or_else(|| {
        let p = pdf?;
        lj::from_pdf(&model, year, &period, p)
            .filter(|b| lj::plausible_pdf(&model, b))
            .map(|b| (b, Source::Pdf, p))
    });
    let Some((boxes, source, bytes)) = read else {
        return Ok(());
    };
    conn.execute(
        "UPDATE aeat_filing SET boxes = ?1, boxes_source = ?2 WHERE id = ?3",
        params![
            serde_json::to_string(&boxes).unwrap_or_else(|_| "{}".into()),
            source.code(),
            id
        ],
    )?;
    // Los modelos que TaxPain compara, y el 200 leído entero (base de los 202 siguientes), como
    // modelo presentado (Importar → Comparar).
    // Las informativas que se comparan (347, 190, 180, 193), con su fichero: sus totales en la
    // [02] y la [03] y, al comparar, sus perceptores del fichero guardado.
    let informativa =
        source == Source::Informative && presentados::INFORMATIVAS.contains(&model.as_str());
    let as_filed = presentados::MODELS.contains(&model.as_str())
        || (model == "200" && source == Source::File)
        || informativa;
    if as_filed && (source != Source::Informative || informativa) {
        // Si ya estaba como modelo presentado (leído antes del PDF), se sustituye: no quedan dos.
        let previous: Option<i64> = conn
            .query_row(
                "SELECT filed_model_id FROM aeat_filing WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let r = presentados::Read {
            model: model.clone(),
            year,
            period: period.clone(),
            source: if source == Source::File || informativa {
                "fichero"
            } else {
                "pdf"
            }
            .into(),
            boxes: boxes
                .casillas
                .iter()
                .filter_map(|(k, v)| Some((u16::try_from(*k).ok()?, *v)))
                // Informativas: importe total en la [02] y retenciones en la [03].
                .chain(boxes.importe.filter(|_| informativa).map(|i| (2, i)))
                .chain(boxes.retenciones.filter(|_| informativa).map(|r| (3, r)))
                // El 349 del PDF trae sus totales, no casillas: su importe va en la [02] (la
                // que se compara con TaxPain y con el 303).
                .chain(
                    boxes
                        .importe
                        .filter(|_| model == "349" && !boxes.casillas.contains_key(&2))
                        .map(|i| (2, i)),
                )
                .collect(),
            boxes_read: true,
            ..presentados::Read::default()
        };
        let ext = if source == Source::File || informativa {
            "txt"
        } else {
            "pdf"
        };
        let filed = presentados::store_read(v, &format!("{base}.{ext}"), bytes, &r, false, now)?;
        conn.execute(
            "UPDATE aeat_filing SET filed_model_id = ?1 WHERE id = ?2",
            params![filed.id(), id],
        )?;
        if let Some(old) = previous
            && old != filed.id()
        {
            presentados::delete(v, old)?;
        }
    }
    Ok(())
}

/// Estado de un modelo y periodo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Status {
    /// La AEAT la tiene.
    Presented,
    /// Tocaba seguro (o TaxPain calcula importes), el plazo acabó y la AEAT no la tiene.
    Missing,
    /// Tocaba y aún está en plazo.
    Open,
    /// La AEAT no la tiene, pero sólo tocaba en ciertos casos (retenciones, pagos fraccionados,
    /// operaciones intracomunitarias…): se enseña con una nota, sin darla por pendiente.
    NotFound,
}

/// Una fila de la tabla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub model: String,
    pub period: String,
    pub status: Status,
    /// Según el perfil fiscal tocaba presentarlo.
    pub expected: bool,
    /// Último día del plazo, si el perfil lo conoce.
    pub due_on: Option<Date>,
    /// Fecha y hora de presentación según la AEAT (la de la declaración vigente).
    pub presented_at: Option<String>,
    /// Si es complementaria o sustitutiva: fecha de la primera declaración del periodo.
    pub original_presented_at: Option<String>,
    /// La primera declaración del periodo se presentó después del último día del plazo.
    pub late: bool,
    /// Es complementaria o sustitutiva y se presentó después del plazo (puede llevar recargo).
    pub late_amendment: bool,
    pub receipt: Option<String>,
    /// Justificantes anteriores a los que sustituye o complementa.
    pub replaces: Vec<String>,
    pub has_pdf: bool,
    /// Se han leído sus casillas (del fichero de la declaración o del PDF).
    pub boxes_read: bool,
    /// Casillas que no cuadran con lo que calcula TaxPain (si se ha podido comparar).
    pub mismatches: Option<usize>,
    /// Modelo guardado para abrir la comparación (Importar → Comparar).
    pub filed_model_id: Option<i64>,
    /// Aviso de esta fila (no se pudo descargar, no se compara, cuándo tocaba…).
    pub note: Option<String>,
}

/// Texto de la AEAT para los modelos que dio por «sin declaraciones».
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AeatAnswer {
    pub text: String,
    pub models: Vec<String>,
}

/// Resultado de la comprobación de un año.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub year: i32,
    pub nif: String,
    /// Cuándo se consultó la AEAT por última vez (`None`: nunca).
    pub checked_at: Option<String>,
    /// Sin perfil fiscal no se sabe qué tocaba: sólo se lista lo presentado.
    pub without_profile: bool,
    pub rows: Vec<Row>,
    /// Modelos consultados de los que la AEAT no tiene nada en el año (y de los que no sale
    /// ninguna fila).
    pub none_filed: Vec<String>,
    /// Modelos que no se pudieron consultar.
    pub errors: Vec<String>,
    /// Con qué texto dijo la AEAT que no había declaraciones.
    pub empty_answers: Vec<AeatAnswer>,
    /// Modelos que el servicio de consulta no permite consultar (hay que mirarlos en la Sede).
    pub unavailable: Vec<String>,
    /// Modelos de los que contestó la AEAT en la última comprobación: los que se consultaron,
    /// sin los que fallaron ni los que el servicio no permite consultar. De los demás, lo que no
    /// sale en la tabla no se sabe.
    pub answered: Vec<String>,
}

impl Report {
    /// Filas que faltan.
    pub fn missing(&self) -> usize {
        self.count(Status::Missing)
    }

    /// Filas con un estado.
    pub fn count(&self, s: Status) -> usize {
        self.rows.iter().filter(|r| r.status == s).count()
    }
}

struct Filing {
    model: String,
    period: String,
    receipt: String,
    previous: Option<String>,
    presented_at: String,
    has_pdf: bool,
    filed_model_id: Option<i64>,
    download_error: Option<String>,
    /// De dónde salen sus casillas («fichero», «informativa», «pdf»), si se han leído.
    boxes_source: Option<String>,
    visor_state: Option<String>,
    /// Las casillas leídas (`FilingBoxes` en JSON).
    boxes: Option<String>,
}

fn filings(v: &Vault, year: i32, nif: Option<&str>) -> Result<Vec<Filing>> {
    let mut stmt = v.connection().prepare(
        "SELECT model, period, receipt, previous_receipt, presented_at, document_id IS NOT NULL,
                filed_model_id, download_error, boxes_source, visor_state, boxes
         FROM aeat_filing WHERE year = ?1 AND (?2 IS NULL OR nif = ?2)
         ORDER BY presented_at, id",
    )?;
    let rows = stmt.query_map(params![year, nif], |r| {
        Ok(Filing {
            model: r.get(0)?,
            period: r.get(1)?,
            receipt: r.get(2)?,
            previous: r.get(3)?,
            presented_at: r.get(4)?,
            has_pdf: r.get(5)?,
            filed_model_id: r.get(6)?,
            download_error: r.get(7)?,
            boxes_source: r.get(8)?,
            visor_state: r.get(9)?,
            boxes: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// ¿Falló la consulta de `model`? (Los errores se guardan como «modelo: mensaje».)
fn failed(c: &Check, model: &str) -> bool {
    c.errors
        .iter()
        .any(|e| e.split(':').next().map(str::trim) == Some(model))
}

/// «21/04/2025».
fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Fecha de un «2025-04-21T10:00:00» o «2025-04-21 10:00:00».
fn date_of(s: &str) -> Option<Date> {
    Date::parse(
        s.get(..10)?,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()
}

/// Clase de periodo: trimestres, meses, pagos fraccionados y anual.
fn period_class(p: &str) -> u8 {
    match p.chars().nth(1) {
        Some('T') => 0,
        Some('P') => 2,
        Some('A') => 3,
        _ => 1,
    }
}

/// Orden de los periodos dentro de un modelo.
fn period_key(p: &str) -> (u8, String) {
    (period_class(p), p.to_owned())
}

/// ¿Tocaba seguro presentarlo en cada periodo que da el perfil? Los demás modelos sólo tocan si
/// hubo retenciones, pagos u operaciones en el periodo. `over_6m`: el ajuste de los modelos «cifra
/// de negocios de más de 6 M€» (el del 202).
fn certain(model: &str, p: &TaxpayerProfile, over_6m: bool) -> bool {
    match model {
        "303" | "390" | "130" | "131" => true,
        "200" => p.persona == Persona::Juridica,
        // `TODO(VERIFY)`: la Renta conjunta la presenta el primer declarante; con el NIF del
        // segundo puede no salir (por eso la fila lleva una nota).
        "100" => p.persona == Persona::Fisica,
        // `TODO(VERIFY)`: según las instrucciones del modelo 202, las sociedades con cifra de
        // negocios de más de 6 M€ en los 12 meses anteriores al inicio del periodo lo presentan
        // aunque no salga nada a ingresar. El ajuste es uno solo, no uno por ejercicio: para un
        // año pasado puede no corresponder.
        "202" => p.persona == Persona::Juridica && over_6m,
        _ => false,
    }
}

/// Retenciones de un trimestral (111, 115 o 123) leídas de su declaración vigente.
fn withheld(model: &str, boxes: Option<&str>) -> bool {
    let Some(b) = boxes
        .and_then(|j| serde_json::from_str::<crate::lectura_justificante::FilingBoxes>(j).ok())
    else {
        return false;
    };
    let any = |cs: &[u32]| cs.iter().any(|c| !b.get(*c).is_zero());
    match model {
        "111" => any(&[28, 30]),
        "115" => any(&[3]),
        // Hasta 2023 la retención va en la [03]; desde 2024, en la [07] y la [09].
        "123" => any(&[3, 7, 9]),
        _ => false,
    }
}

/// Cuándo hacía falta un modelo que no es obligatorio en todos los periodos.
fn when_needed(model: &str) -> &'static str {
    match model {
        "111" => {
            "Sólo hacía falta si hubo pagos con retención (nóminas, profesionales, \
                   administradores…) en el periodo."
        }
        "115" => "Sólo hacía falta si se pagó el alquiler de un local con retención.",
        "123" => {
            "Sólo hacía falta si se pagaron dividendos, intereses u otras rentas del capital con \
             retención."
        }
        "202" => {
            "Sólo hacía falta si salía pago fraccionado a ingresar (las grandes empresas, \
                   siempre)."
        }
        "349" => "Sólo hacía falta si hubo operaciones intracomunitarias en el periodo.",
        "180" | "190" | "193" | "296" => {
            "Resumen anual: sólo hacía falta si hubo retenciones de ese tipo en el año."
        }
        "347" => "Sólo hacía falta si hubo operaciones de más de 3.005,06 € con alguien.",
        "232" => "Sólo hacía falta si hubo operaciones vinculadas por encima de los umbrales.",
        "720" | "721" => {
            "Sólo hacía falta si hubo bienes o monedas virtuales en el extranjero por encima de \
             los umbrales."
        }
        _ => "Sólo hacía falta en ciertos casos.",
    }
}

/// ¿Calcula TaxPain importes de este modelo y periodo? (Si sí, tocaba presentarlo.)
fn taxpain_has_amounts(v: &Vault, model: &str, year: i32, period: &str) -> bool {
    if !presentados::MODELS.contains(&model) {
        return false;
    }
    let Ok(q) = presentados::quarter_of(model, period) else {
        return false;
    };
    let Ok(d) = crate::models::draft(v, model, year, q) else {
        return false;
    };
    let amounts = presentados::known_boxes(model);
    !d.result.is_zero()
        || !d.details.is_empty()
        || d.boxes
            .iter()
            .any(|(b, m)| amounts.contains(b) && !m.is_zero())
}

/// La tabla del año con lo guardado (no consulta la AEAT).
pub fn report(v: &Vault, year: i32, today: Date) -> Result<Report> {
    report_for(v, year, today, profile(v)?)
}

/// Como [`report`], con el perfil fiscal que se le dé (el deducido de lo presentado, en la
/// revisión de una empresa sin perfil guardado).
pub(crate) fn report_for(
    v: &Vault,
    year: i32,
    today: Date,
    profile: Option<TaxpayerProfile>,
) -> Result<Report> {
    let own = company_nif(v);
    // Una comprobación hecha con otro NIF no es de esta empresa.
    let check = checks(v)?
        .remove(&year)
        .filter(|c| own.as_ref().is_none_or(|n| *n == c.nif));
    let exp = profile
        .as_ref()
        .map(|p| expected(p, year))
        .unwrap_or_default();
    let all = filings(v, year, own.as_deref())?;
    let replaced: BTreeSet<&str> = all.iter().filter_map(|f| f.previous.as_deref()).collect();

    let mut rows: Vec<Row> = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for f in all
        .iter()
        .filter(|f| !replaced.contains(f.receipt.as_str()))
    {
        let key = (f.model.clone(), f.period.clone());
        seen.insert(key.clone());
        let due_on = exp.get(&key).copied();
        // La cadena de justificantes a los que sustituye, hasta la original.
        let mut replaces = Vec::new();
        let mut original = f;
        let mut prev = f.previous.clone();
        while let Some(p) = prev {
            if replaces.contains(&p) {
                break;
            }
            let g = all.iter().find(|g| g.receipt == p);
            if let Some(g) = g {
                original = g;
            }
            prev = g.and_then(|g| g.previous.clone());
            replaces.push(p);
        }
        let mut note = f.download_error.clone();
        let mut mismatches = None;
        if let Some(id) = f.filed_model_id
            && presentados::comparable(&f.model)
        {
            match presentados::compare(v, id) {
                Ok(c) => mismatches = Some(c.mismatches),
                Err(e) => {
                    note.get_or_insert_with(|| format!("No se pudo comparar con TaxPain: {e}"));
                }
            }
        }
        let boxes_read = f.boxes_source.is_some() || f.filed_model_id.is_some();
        if note.is_none() {
            note = if !boxes_read {
                Some(if !f.has_pdf {
                    "Sin justificante: se reintenta al volver a comprobar.".to_owned()
                } else if f.visor_state.as_deref() == Some("diseno_desconocido") {
                    "TaxPain aún no tiene el diseño de registro de este ejercicio y no se han \
                     podido leer las casillas del PDF: actualiza TaxPain o revisa el justificante."
                        .to_owned()
                } else if f.visor_state.is_some() {
                    "La AEAT no da el fichero de esta declaración y no se han podido leer las \
                     casillas de su PDF: revisa el justificante."
                        .to_owned()
                } else {
                    "No se pudieron leer las casillas: se reintenta al volver a comprobar."
                        .to_owned()
                })
            } else if !presentados::comparable(&f.model) {
                Some(match (f.model.as_str(), f.boxes_source.as_deref()) {
                    (_, Some("informativa")) => {
                        "Se leen los totales (registros, importe y retenciones): TaxPain no \
                         compara este modelo."
                            .to_owned()
                    }
                    _ => "TaxPain no compara este modelo: sus casillas se usan en la evolución \
                          de la empresa."
                        .to_owned(),
                })
            } else if f.boxes_source.as_deref() == Some("pdf")
                && f.visor_state.as_deref() == Some("diseno_desconocido")
            {
                Some(
                    "TaxPain aún no tiene el diseño de registro de este ejercicio: casillas \
                     leídas del PDF (aproximado). Actualiza TaxPain."
                        .to_owned(),
                )
            } else if f.boxes_source.as_deref() == Some("pdf") {
                Some("Casillas leídas del PDF del justificante (aproximado).".to_owned())
            } else {
                None
            };
        }
        let late_on = |at: &str| match (due_on, date_of(at)) {
            (Some(d), Some(p)) => p > d,
            _ => false,
        };
        let amended = !replaces.is_empty();
        rows.push(Row {
            model: f.model.clone(),
            period: f.period.clone(),
            status: Status::Presented,
            expected: due_on.is_some(),
            due_on,
            presented_at: Some(f.presented_at.clone()),
            // Si la original no está en la AEAT (o no se guardó), no se sabe su fecha.
            original_presented_at: (amended && !std::ptr::eq(original, f))
                .then(|| original.presented_at.clone()),
            // Si la original no está en la lista de la AEAT, no se sabe cuándo se presentó: no se
            // dice que la primera presentación fuera tarde.
            late: (!amended || !std::ptr::eq(original, f)) && late_on(&original.presented_at),
            late_amendment: amended && late_on(&f.presented_at),
            receipt: Some(f.receipt.clone()),
            replaces,
            has_pdf: f.has_pdf,
            boxes_read,
            mismatches,
            filed_model_id: f.filed_model_id,
            note,
        });
    }
    // Lo que tocaba y la AEAT no tiene. Sin consultar la AEAT no se sabe: sólo se dice tras una
    // comprobación, y sólo de los modelos que se consultaron.
    if let (Some(c), Some(p)) = (&check, &profile) {
        let checked_on = date_of(&c.checked_at);
        let over_6m = crate::models::settings(v).is_ok_and(|s| s.turnover_over_6m);
        // Antes del alta no hay obligaciones. TaxPain no guarda la fecha de alta: un año sin
        // ninguna declaración en la AEAT, o los periodos anteriores a la primera presentada en
        // el año, salen como «No consta» con una pregunta, no como «Falta».
        let first_index = all
            .iter()
            .filter(|f| f.period != "0A")
            .filter_map(|f| taxpain_models::dr_versions::period_index(&f.period))
            .min();
        let no_filings = all.is_empty() && c.errors.is_empty();
        // Trimestrales vigentes con retenciones: tocaba su resumen anual.
        let retained: BTreeSet<&str> = all
            .iter()
            .filter(|f| !replaced.contains(f.receipt.as_str()))
            .filter(|f| withheld(&f.model, f.boxes.as_deref()))
            .map(|f| f.model.as_str())
            .collect();
        // Clases de periodo que tiene la AEAT de cada modelo (trimestral, mensual…).
        let mut classes: BTreeMap<&str, BTreeSet<u8>> = BTreeMap::new();
        for (m, per) in &seen {
            classes
                .entry(m.as_str())
                .or_default()
                .insert(period_class(per));
        }
        for ((model, period), due_on) in &exp {
            // Si la consulta de ese modelo falló, no se sabe si falta.
            if seen.contains(&(model.clone(), period.clone()))
                || !c.models.contains(model)
                || failed(c, model)
                || c.unavailable.contains_key(model)
            {
                continue;
            }
            let base = Row {
                model: model.clone(),
                period: period.clone(),
                status: Status::Open,
                expected: true,
                due_on: Some(*due_on),
                presented_at: None,
                original_presented_at: None,
                late: false,
                late_amendment: false,
                receipt: None,
                replaces: Vec::new(),
                has_pdf: false,
                boxes_read: false,
                mismatches: None,
                filed_model_id: None,
                note: None,
            };
            if *due_on >= today {
                rows.push(base);
                continue;
            }
            // Si su plazo acabó después de la última comprobación, lo que contestó la AEAT
            // entonces no dice si se presentó.
            if let Some(on) = checked_on.filter(|on| due_on >= on) {
                rows.push(Row {
                    status: Status::NotFound,
                    note: Some(format!(
                        "Su plazo acabó el {}, después de la última comprobación ({}): vuelve a \
                         comprobar para saber si se presentó.",
                        dmy(*due_on),
                        dmy(on)
                    )),
                    ..base
                });
                continue;
            }
            let other_class = classes
                .get(model.as_str())
                .is_some_and(|k| !k.contains(&period_class(period)));
            let informative_unseen =
                INFORMATIVE.contains(&model.as_str()) && !classes.contains_key(model.as_str());
            let summary_of = match model.as_str() {
                "190" => Some("111"),
                "180" => Some("115"),
                "193" => Some("123"),
                _ => None,
            };
            let before_first = period != "0A"
                && first_index.is_some_and(|first| {
                    taxpain_models::dr_versions::period_index(period).is_some_and(|i| i < first)
                });
            let (status, note) = if no_filings {
                (
                    Status::NotFound,
                    "La AEAT no tiene ninguna declaración de la empresa en este año: ¿estaba ya \
                     dada de alta? Si lo estaba, esta declaración falta."
                        .to_owned(),
                )
            } else if before_first {
                (
                    Status::NotFound,
                    "Es anterior a la primera declaración de la empresa en este año: ¿estaba ya \
                     dada de alta? Si lo estaba, esta declaración falta."
                        .to_owned(),
                )
            } else if other_class {
                (
                    Status::NotFound,
                    "La AEAT tiene este modelo con otra periodicidad ese año: revisa el perfil \
                     fiscal."
                        .to_owned(),
                )
            } else if certain(model, p, over_6m) {
                let note = if model == "100" {
                    "Si la Renta fue conjunta y la presentó tu cónyuge como primer declarante, \
                     puede no salir con tu NIF: compruébalo en la Sede."
                } else {
                    ""
                };
                (Status::Missing, note.to_owned())
            } else if let Some(q) = summary_of
                && retained.contains(q)
            {
                (
                    Status::Missing,
                    format!("Hubo retenciones en los {q} de este año: tocaba su resumen anual."),
                )
            } else if taxpain_has_amounts(v, model, year, period) {
                (
                    Status::Missing,
                    "TaxPain calcula importes en este periodo, así que tocaba presentarlo."
                        .to_owned(),
                )
            } else if informative_unseen && !matches!(model.as_str(), "190" | "193" | "347" | "349")
            {
                (
                    Status::NotFound,
                    format!(
                        "{} No se ha comprobado que esta consulta de la AEAT liste este modelo: \
                         compruébalo en la Sede.",
                        when_needed(model)
                    ),
                )
            } else {
                (Status::NotFound, when_needed(model).to_owned())
            };
            rows.push(Row {
                status,
                note: (!note.is_empty()).then_some(note),
                ..base
            });
        }
    }
    rows.sort_by(|a, b| {
        (order(&a.model), period_key(&a.period), &a.presented_at).cmp(&(
            order(&b.model),
            period_key(&b.period),
            &b.presented_at,
        ))
    });
    let with_rows: BTreeSet<&str> = rows.iter().map(|r| r.model.as_str()).collect();
    let none_filed = check
        .as_ref()
        .map(|c| {
            c.models
                .iter()
                .filter(|m| {
                    !with_rows.contains(m.as_str())
                        && !failed(c, m)
                        && !c.unavailable.contains_key(m.as_str())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut answers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(c) = &check {
        for (m, t) in &c.empty {
            answers.entry(t.clone()).or_default().push(m.clone());
        }
    }
    Ok(Report {
        year,
        nif: check
            .as_ref()
            .map(|c| c.nif.clone())
            .or(own)
            .unwrap_or_default(),
        checked_at: check.as_ref().map(|c| c.checked_at.clone()),
        without_profile: profile.is_none(),
        rows,
        none_filed,
        answered: check
            .as_ref()
            .map(|c| {
                c.models
                    .iter()
                    .filter(|m| !failed(c, m) && !c.unavailable.contains_key(m.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        unavailable: check
            .as_ref()
            .map(|c| {
                let mut m: Vec<String> = c.unavailable.keys().cloned().collect();
                m.sort_by_key(|x| order(x));
                m
            })
            .unwrap_or_default(),
        errors: check.map(|c| c.errors).unwrap_or_default(),
        empty_answers: answers
            .into_iter()
            .map(|(text, mut models)| {
                models.sort_by_key(|m| order(m));
                AeatAnswer { text, models }
            })
            .collect(),
    })
}

/// Guarda en `dir` los justificantes en PDF del año. Devuelve las rutas escritas.
pub fn export(v: &Vault, year: i32, dir: &Path) -> Result<Vec<String>> {
    let mut stmt = v.connection().prepare(
        "SELECT f.model, f.period, f.receipt, d.content FROM aeat_filing f
         JOIN document d ON d.id = f.document_id
         WHERE f.year = ?1 AND (?2 IS NULL OR f.nif = ?2)
         ORDER BY f.model, f.period, f.presented_at",
    )?;
    let rows: Vec<(String, String, String, Vec<u8>)> = stmt
        .query_map(params![year, company_nif(v)], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    };
    let mut out = Vec::new();
    for (model, period, receipt, content) in rows {
        let path = dir.join(format!(
            "Modelo_{}_{year}_{}_justificante_{}.pdf",
            safe(&model),
            safe(&period),
            safe(&receipt)
        ));
        std::fs::write(&path, content)
            .map_err(|e| AppError::Other(format!("no se pudo guardar {}: {e}", path.display())))?;
        out.push(path.display().to_string());
    }
    Ok(out)
}

/// Guarda los justificantes de varios años, cada año en su subcarpeta de `dir` (sólo los años
/// que tienen alguno). Devuelve las rutas escritas.
pub fn export_years(v: &Vault, years: &[i32], dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for &y in years {
        let has: bool = v.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM aeat_filing
                           WHERE year = ?1 AND document_id IS NOT NULL AND (?2 IS NULL OR nif = ?2))",
            params![y, company_nif(v)],
            |r| r.get(0),
        )?;
        if !has {
            continue;
        }
        let sub: PathBuf = dir.join(y.to_string());
        std::fs::create_dir_all(&sub)
            .map_err(|e| AppError::Other(format!("no se pudo crear {}: {e}", sub.display())))?;
        out.extend(export(v, y, &sub)?);
    }
    Ok(out)
}

/// Una declaración vigente según la AEAT (no sustituida por una complementaria o sustitutiva),
/// con sus casillas si se han leído.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vigente {
    pub model: String,
    pub year: i32,
    pub period: String,
    pub receipt: String,
    pub presented_at: String,
    pub boxes: Option<crate::lectura_justificante::FilingBoxes>,
    /// «fichero», «informativa» o «pdf».
    pub boxes_source: Option<String>,
    /// El justificante al que complementa o sustituye, si es una complementaria o sustitutiva.
    pub previous: Option<String>,
    /// Cuándo se presentó la primera de la cadena (la original), si es otra y la AEAT la tiene.
    pub original_presented_at: Option<String>,
}

/// Las declaraciones vigentes de la empresa que tiene la AEAT (de todas las comprobaciones), por
/// ejercicio, modelo y periodo.
pub fn vigentes(v: &Vault) -> Result<Vec<Vigente>> {
    let nif = company_nif(v);
    let mut stmt = v.connection().prepare(
        "SELECT a.model, a.year, a.period, a.receipt, a.presented_at, a.boxes, a.boxes_source,
                a.previous_receipt
         FROM aeat_filing a
         WHERE (?1 IS NULL OR a.nif = ?1)
           AND NOT EXISTS (SELECT 1 FROM aeat_filing b
                           WHERE b.previous_receipt = a.receipt
                             AND b.model = a.model AND b.year = a.year)
         ORDER BY a.year, a.model, a.period, a.presented_at",
    )?;
    let rows = stmt.query_map(params![nif], |r| {
        let boxes: Option<String> = r.get(5)?;
        Ok(Vigente {
            model: r.get(0)?,
            year: r.get(1)?,
            period: r.get(2)?,
            receipt: r.get(3)?,
            presented_at: r.get(4)?,
            boxes: boxes.and_then(|b| serde_json::from_str(&b).ok()),
            boxes_source: r.get(6)?,
            previous: r.get(7)?,
            original_presented_at: None,
        })
    })?;
    let mut out: Vec<Vigente> = rows.collect::<std::result::Result<_, _>>()?;
    // La fecha de la original: la cadena de justificantes anteriores, hasta el primero.
    let mut anterior = v.connection().prepare(
        "SELECT previous_receipt, presented_at FROM aeat_filing
         WHERE receipt = ?1 AND model = ?2 AND year = ?3 AND (?4 IS NULL OR nif = ?4)",
    )?;
    for x in &mut out {
        let mut vistos = BTreeSet::new();
        let mut prev = x.previous.clone();
        while let Some(p) = prev.take() {
            if !vistos.insert(p.clone()) {
                break;
            }
            let fila: Option<(Option<String>, String)> = anterior
                .query_row(params![p, x.model, x.year, nif], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            if let Some((antes, fecha)) = fila {
                x.original_presented_at = Some(fecha);
                prev = antes;
            }
        }
    }
    Ok(out)
}

/// Resumen de un año (para «Comprobar varios años»).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct YearSummary {
    pub year: i32,
    pub presented: usize,
    pub missing: usize,
    pub not_found: usize,
    pub open: usize,
    pub pdfs: usize,
    /// Declaraciones presentadas con sus casillas leídas.
    pub boxes_read: usize,
    /// Presentadas fuera de plazo (la primera del periodo).
    pub late: usize,
    /// Modelos que no se pudieron consultar.
    pub errors: usize,
    /// Los mismos, con el mensaje («modelo: mensaje»).
    pub failed: Vec<String>,
    /// Modelos que el servicio no permite consultar (hay que mirarlos en la Sede).
    pub unavailable: Vec<String>,
    /// Cuándo se consultó.
    pub checked_at: Option<String>,
}

impl From<&Report> for YearSummary {
    fn from(r: &Report) -> Self {
        let presented = |x: &&Row| x.status == Status::Presented;
        YearSummary {
            year: r.year,
            presented: r.count(Status::Presented),
            missing: r.count(Status::Missing),
            not_found: r.count(Status::NotFound),
            open: r.count(Status::Open),
            pdfs: r.rows.iter().filter(|x| x.has_pdf).count(),
            boxes_read: r
                .rows
                .iter()
                .filter(presented)
                .filter(|x| x.boxes_read)
                .count(),
            late: r.rows.iter().filter(presented).filter(|x| x.late).count(),
            errors: r.errors.len(),
            failed: r.errors.clone(),
            unavailable: r.unavailable.clone(),
            checked_at: r.checked_at.clone(),
        }
    }
}

/// Años comprobados con la AEAT (con el NIF de la empresa), del más antiguo al más reciente.
pub fn checked_years(v: &Vault) -> Result<Vec<i32>> {
    let own = company_nif(v);
    Ok(checks(v)?
        .into_iter()
        .filter(|(_, c)| own.as_ref().is_none_or(|n| *n == c.nif))
        .map(|(y, _)| y)
        .collect())
}

/// El resumen de todos los años comprobados (sin consultar la AEAT).
pub fn summaries(v: &Vault, today: Date) -> Result<Vec<YearSummary>> {
    checked_years(v)?
        .into_iter()
        .map(|y| report(v, y, today).map(|r| YearSummary::from(&r)))
        .collect()
}

/// Fecha y hora de la última comprobación con la AEAT de la empresa (de cualquier año), o
/// `None` si nunca se ha comprobado.
pub fn last_check(v: &Vault) -> Result<Option<String>> {
    let own = company_nif(v);
    Ok(checks(v)?
        .into_values()
        .filter(|c| own.as_ref().is_none_or(|n| *n == c.nif))
        .map(|c| c.checked_at)
        .max())
}

/// ¿Tiene la AEAT una declaración de ese modelo y periodo, según la última comprobación? (Para
/// no dar por vencido sin presentar lo que presentó la gestoría.)
pub fn filed_at_aeat(v: &Vault, model: &str, year: i32, period: &str) -> Result<bool> {
    Ok(v.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM aeat_filing
                       WHERE model = ?1 AND year = ?2 AND period = ?3 AND (?4 IS NULL OR nif = ?4))",
        params![model, year, period, company_nif(v)],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nif_de_persona_fisica() {
        assert!(natural_person_nif("12345678Z"));
        assert!(natural_person_nif("x1234567l"));
        assert!(!natural_person_nif("B12345674"));
    }

    /// La declaración sustituida por una complementaria no es «lo presentado» en los cálculos,
    /// aunque se haya importado después (id mayor).
    #[test]
    fn la_sustituida_no_manda_en_los_calculos() {
        use std::sync::Arc;
        use taxpain_core::money::Money;
        use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
        let v = Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap();
        let now = time::macros::datetime!(2026-10-27 10:00 UTC);
        let p = |csv: &str, receipt: &str, prev: Option<&str>| Presented {
            model: "303".into(),
            year: 2025,
            period: "2T".into(),
            csv: csv.into(),
            file_number: String::new(),
            receipt: receipt.into(),
            previous_receipt: prev.map(Into::into),
            presented_at: "2025-07-15T10:00:00".into(),
        };
        let a = Answer {
            models: vec!["303".into()],
            list: vec![
                p("AAAAAAAAAAAAAAA1", "3030000000001", None),
                p("AAAAAAAAAAAAAAA2", "3030000000002", Some("3030000000001")),
            ],
            ..Answer::default()
        };
        record(&v, "B12345674", 2025, &a, now).unwrap();
        let store = |name: &str, total: i64, csv: &str| {
            let r = presentados::Read {
                model: "303".into(),
                year: 2025,
                period: "2T".into(),
                source: "fichero".into(),
                boxes: [(71, Money::from_euros(total, 0))].into_iter().collect(),
                boxes_read: true,
                ..presentados::Read::default()
            };
            let id = presentados::store_read(&v, name, name.as_bytes(), &r, false, now)
                .unwrap()
                .id();
            v.connection()
                .execute(
                    "UPDATE aeat_filing SET filed_model_id = ?1 WHERE csv = ?2",
                    params![id, csv],
                )
                .unwrap();
        };
        // Primero la complementaria y después la original.
        store("complementaria", 300, "AAAAAAAAAAAAAAA2");
        store("original", 100, "AAAAAAAAAAAAAAA1");
        let (_, boxes) = presentados::filed_by_gestor(&v, "303", 2025, "2T")
            .unwrap()
            .unwrap();
        assert_eq!(boxes.get(&71), Some(&Money::from_euros(300, 0)));
    }

    #[test]
    fn clases_de_periodo() {
        assert_eq!(period_class("1T"), 0);
        assert_eq!(period_class("07"), 1);
        assert_eq!(period_class("2P"), 2);
        assert_eq!(period_class("0A"), 3);
    }
}
