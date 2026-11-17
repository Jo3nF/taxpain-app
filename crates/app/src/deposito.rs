//! Depósito de cuentas anuales en el Registro Mercantil: paquete para **importar en el programa
//! D2** del Colegio de Registradores (vía A de `docs/research/DEPOSITO_CUENTAS.md`).
//!
//! TaxPain genera un ZIP con `DEPOSITO.XML` (esquema del Anexo II de la Resolución de la DGSJFP
//! de 19/5/2026, en `fixtures/deposito/DEPOSITO.xsd`) con todas las claves que salen de la
//! contabilidad: identificación, balance y cuenta de pérdidas y ganancias del ejercicio y del
//! anterior (código + `9`) con los códigos de partida de cada modelo, aplicación del resultado y
//! las hojas sencillas (medioambiente y autocartera «sin»), más la memoria en PDF si se aporta.
//! En el D2 se importa, se completa lo que falte (titular real, firmantes…), se valida y el D2
//! genera el XBRL, el ZIP definitivo y su huella para la certificación. TaxPain no presenta nada.
//!
//! `TODO(VERIFY)` (falta un ZIP real del D2 2025 para comparar): códigos con cero inicial
//! (`01010`), qué clave es la fecha de inicio y cuál la de cierre (01102/01101), valor de la
//! provincia (01025) y la cadena 8081010.

use std::fmt::Write as _;
use std::io::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::accounting::statements::{ModelKind, Statement};
use taxpain_core::invoice::PartyId;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::OptionalExtension;
use time::Date;

use crate::accounting;
use crate::error::{AppError, Result};

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegalForm {
    Sa,
    #[default]
    Sl,
    Otra,
}

/// Datos del depósito que no salen de la contabilidad.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub year: i32,
    pub model: ModelKind,
    /// Código del Registro Mercantil (5 dígitos; p. ej. 46030 Valencia).
    pub registry_code: String,
    pub legal_form: LegalForm,
    /// Actividad en texto y CNAE-2025.
    pub activity: String,
    pub cnae: String,
    /// Domicilio (si no está en los datos de facturación).
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub town: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    /// Personal medio del ejercicio: fijo y no fijo.
    #[serde(default)]
    pub staff_permanent: Option<String>,
    #[serde(default)]
    pub staff_temporary: Option<String>,
    /// Aplica los criterios específicos de microempresas (clave 01902).
    #[serde(default)]
    pub micro_criteria: bool,
    /// Administradores que firman las cuentas.
    #[serde(default)]
    pub signers: Vec<String>,
    /// Memoria en PDF (opcional).
    #[serde(default, skip_serializing)]
    pub memoria_pdf: Option<Vec<u8>>,
    /// Sin PDF adjunto, incluir la memoria que prepara TaxPain con el modelo del depósito.
    #[serde(default)]
    pub memoria_taxpain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Package {
    pub filename: String,
    #[serde(skip)]
    pub zip: Vec<u8>,
    /// SHA-256 del ZIP en Base64 (la huella definitiva será la del ZIP que genere el D2).
    pub huella: String,
    /// Número de claves escritas.
    pub claves: usize,
    pub warnings: Vec<String>,
}

/// Importe con punto decimal y dos decimales, sin separador de miles.
fn amount(m: Money) -> String {
    let c = m.cents();
    let sign = if c < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        c.unsigned_abs() / 100,
        c.unsigned_abs() % 100
    )
}

fn dd_mm_aaaa(d: Date) -> String {
    format!("{:02}.{:02}.{:04}", d.day(), u8::from(d.month()), d.year())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Cadena 8081010 (24 posiciones) de los documentos que acompañan.
fn documents_code(model: ModelKind, memoria: bool) -> String {
    let m = match model {
        ModelKind::Normal => '1',
        ModelKind::Abreviado => '2',
        ModelKind::Pymes => '3',
    };
    let memo = if memoria { m } else { '0' };
    // Posición a posición (Anexo II): a balance, b PyG, c memoria, d ECPN, e EFE,
    // f hoja de identificación, g gestión, h auditoría, i autocartera, j convocatoria, k SICAV,
    // l certificación, m moneda (E), nn nº de «otros», o otros, p medioambiente,
    // q no financiera, s titular real, t servicios a terceros, r retención, u ESEF, v COVID,
    // w informe por países.
    let parts: [&str; 23] = [
        &m.to_string(),
        &m.to_string(),
        &memo.to_string(),
        "0",
        "0",
        "1",
        "0",
        "0",
        "1",
        "0",
        "0",
        "1",
        "E",
        "00",
        "0",
        "1",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
    ];
    parts.concat()
}

/// Claves de un estado (ejercicio actual o, con `suffix`, el anterior), con importe distinto de
/// cero.
fn statement_claves(out: &mut Vec<(String, String)>, st: &Statement, suffix: &str) {
    for l in &st.lines {
        if let Some(code) = &l.code
            && !l.amount.is_zero()
        {
            out.push((format!("{code}{suffix}"), amount(l.amount)));
        }
    }
}

/// Prepara el ZIP del depósito para importarlo en el D2.
pub fn build(v: &Vault, r: &Request) -> Result<Package> {
    // La memoria que prepara TaxPain (borrador con corchetes: avisa) si no se adjunta otra.
    if r.memoria_pdf.is_none() && r.memoria_taxpain {
        let mut con = r.clone();
        con.memoria_pdf = Some(crate::memoria::pdf_modelo(v, r.year, r.model)?);
        con.memoria_taxpain = false;
        let mut p = build(v, &con)?;
        p.warnings.push(
            "la memoria incluida es el borrador de TaxPain: si lleva corchetes, complétala y \
             adjunta la versión definitiva en PDF"
                .into(),
        );
        return Ok(p);
    }
    let year = r.year;
    let closed: Option<i64> = v
        .connection()
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [year],
            |row| row.get(0),
        )
        .optional()?;
    let mut warnings = Vec::new();
    if closed != Some(1) {
        warnings.push(format!(
            "el ejercicio {year} no está cerrado: las cuentas pueden cambiar todavía"
        ));
    }
    let registry = r.registry_code.trim();
    if registry.len() != 5 || !registry.bytes().all(|b| b.is_ascii_digit()) {
        return Err(other("el código del Registro Mercantil tiene 5 dígitos"));
    }
    let cnae = r.cnae.trim();
    if cnae.is_empty() {
        return Err(other("falta el código CNAE-2025 (obligatorio)"));
    }
    let inv = crate::invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let nif = match &inv.issuer.id {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    };
    let addr = inv.issuer.address.as_ref();
    let pick = |given: &Option<String>, fallback: Option<&String>| {
        given
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| fallback.cloned())
            .unwrap_or_default()
    };
    let address = pick(&r.address, addr.map(|a| &a.street));
    let town = pick(&r.town, addr.map(|a| &a.town));
    let postal = pick(&r.postal_code, addr.map(|a| &a.post_code));
    if address.is_empty() || town.is_empty() || postal.len() != 5 {
        return Err(other(
            "falta el domicilio de la empresa (calle, municipio y código postal de 5 dígitos)",
        ));
    }

    let now = accounting::annual_accounts_model(v, year, r.model)?;
    if !now.balanced {
        return Err(other(
            "el balance no cuadra (activo distinto de patrimonio neto y pasivo): revisa los asientos",
        ));
    }
    let unmapped: Vec<String> = now
        .balance
        .unmapped
        .iter()
        .chain(&now.income.unmapped)
        .map(|b| b.account.as_str().to_owned())
        .collect();
    if !unmapped.is_empty() {
        warnings.push(format!(
            "cuentas con saldo sin partida en el modelo: {}",
            unmapped.join(", ")
        ));
    }
    let previous = if accounting::journal(v, year - 1)?.is_empty() {
        None
    } else {
        Some(accounting::annual_accounts_model(v, year - 1, r.model)?)
    };

    let fy = accounting::fiscal_year(v, year)?;
    let (start, end) = (fy.start, fy.end);
    let prev_fy = accounting::fiscal_year(v, year - 1)?;
    let mut c: Vec<(String, String)> = Vec::new();
    let mut put = |k: &str, val: String| {
        if !val.trim().is_empty() {
            c.push((k.to_owned(), val));
        }
    };
    // Identificación (hoja de datos generales).
    put("01010", nif.clone());
    put(
        match r.legal_form {
            LegalForm::Sa => "01011",
            LegalForm::Sl => "01012",
            LegalForm::Otra => "01013",
        },
        "1".into(),
    );
    put("01020", inv.issuer.name.clone());
    put("01022", address);
    put("01023", town);
    put("01024", postal.clone());
    put("01025", postal[..2].to_owned());
    put("02009", r.activity.trim().to_owned());
    put("02014", cnae.to_owned());
    put("01102", dd_mm_aaaa(start));
    put("01101", dd_mm_aaaa(end));
    if previous.is_some() {
        put("011029", dd_mm_aaaa(prev_fy.start));
        put("011019", dd_mm_aaaa(prev_fy.end));
    }
    put("01902", if r.micro_criteria { "1" } else { "0" }.into());
    put("09001", "1".into());
    if let Some(s) = &r.staff_permanent {
        put("04001", s.trim().replace(',', "."));
    }
    if let Some(s) = &r.staff_temporary {
        put("04002", s.trim().replace(',', "."));
    }
    let mut claves = c;
    // Balance y PyG (actual y anterior).
    statement_claves(&mut claves, &now.balance, "");
    statement_claves(&mut claves, &now.income, "");
    if let Some(p) = &previous {
        statement_claves(&mut claves, &p.balance, "9");
        statement_claves(&mut claves, &p.income, "9");
    }
    // Aplicación del resultado: con beneficio, la base de reparto es el resultado (PID_033).
    let result = now.income.get(r.model.result_id());
    if result.cents() > 0 {
        claves.push(("91000".into(), amount(result)));
        warnings.push(
            "indica en el D2 cómo se aplica el resultado (reservas, dividendos…): sólo se ha rellenado el total"
                .into(),
        );
    }
    // Solicitud (PR): fecha de cierre, documentos, control.
    claves.push(("8080030".into(), format!("{:02}", end.day())));
    claves.push(("8080020".into(), format!("{:02}", u8::from(end.month()))));
    claves.push(("8080190".into(), year.to_string()));
    let (bal, pyg, memo) = match r.model {
        ModelKind::Normal => ("8080802", "8080804", "8080806"),
        ModelKind::Abreviado => ("8080801", "8080803", "8080805"),
        ModelKind::Pymes => ("8080850", "8080851", "8080852"),
    };
    claves.push((bal.into(), "1".into()));
    claves.push((pyg.into(), "1".into()));
    if r.memoria_pdf.is_some() {
        claves.push((memo.into(), "1".into()));
    } else {
        warnings
            .push("no se adjunta la memoria: añádela en el D2 (o la memoria normalizada)".into());
    }
    if r.model != ModelKind::Pymes {
        warnings.push(
            "el estado de cambios en el patrimonio neto es obligatorio en este modelo: \
             rellénalo en el D2 con el que prepara TaxPain (Contabilidad → Cuentas anuales)"
                .into(),
        );
    }
    claves.push(("8080800".into(), "1".into()));
    claves.push(("8080809".into(), "1".into()));
    claves.push(("8080819".into(), "1".into()));
    claves.push(("8080811".into(), "1".into()));
    claves.push((
        "8081010".into(),
        documents_code(r.model, r.memoria_pdf.is_some()),
    ));
    claves.push((
        "8081997".into(),
        format!(
            "{}{:02}{:02}{:02}",
            nif.chars().take(3).collect::<String>(),
            end.day(),
            u8::from(end.month()),
            year % 100
        ),
    ));
    // Firmantes (H), medioambiente «no hay» (IMA) y autocartera «sin operaciones» (A1).
    if r.signers.is_empty() {
        warnings.push("indica en el D2 los administradores que firman las cuentas".into());
    }
    for (i, s) in r.signers.iter().take(10).enumerate() {
        claves.push((format!("{}", 8_091_001 + i), s.chars().take(40).collect()));
    }
    claves.push(("8099000".into(), "1".into()));
    claves.push(("8009050".into(), "1".into()));
    warnings.push(
        "completa en el D2 la hoja de titular real y revisa la de autocartera y medioambiente"
            .into(),
    );

    let tipo = match r.model {
        ModelKind::Normal => "Normal",
        ModelKind::Abreviado => "Abreviado",
        ModelKind::Pymes => "Pymes",
    };
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Esquema>\n");
    let _ = write!(
        xml,
        "  <Cabecera>\n    <CIF>{}</CIF>\n    <RazonSocial>{}</RazonSocial>\n    \
         <TipoCuestionario>{tipo}</TipoCuestionario>\n    <IdiomaCuestionario>Castellano</IdiomaCuestionario>\n    \
         <MemoriaNormalizada>false</MemoriaNormalizada>\n    <Ejercicio>{year}</Ejercicio>\n  </Cabecera>\n  <Claves>\n",
        esc(&nif),
        esc(&inv.issuer.name),
    );
    for (k, val) in &claves {
        let _ = writeln!(
            xml,
            "    <Clave><Codigo>{k}</Codigo><Valor>{}</Valor></Clave>",
            esc(val)
        );
    }
    xml.push_str("  </Claves>\n</Esquema>\n");

    // ZIP 2.0, Deflate, todo en la raíz.
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(false);
    let mut add = |name: &str, bytes: &[u8]| -> Result<()> {
        zip.start_file(name, opts)
            .map_err(|e| other(format!("ZIP: {e}")))?;
        zip.write_all(bytes)
            .map_err(|e| other(format!("ZIP: {e}")))?;
        Ok(())
    };
    add("DEPOSITO.XML", xml.as_bytes())?;
    if let Some(pdf) = &r.memoria_pdf {
        if pdf.len() > 10 * 1024 * 1024 {
            return Err(other("los adjuntos no pueden pasar de 10 MB"));
        }
        if !pdf.starts_with(b"%PDF-") {
            return Err(other("la memoria ha de ser un PDF"));
        }
        add("MEMORIA.PDF", pdf)?;
    }
    let zip = zip
        .finish()
        .map_err(|e| other(format!("ZIP: {e}")))?
        .into_inner();
    let nif9: String = nif
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(9)
        .collect();
    Ok(Package {
        filename: format!("{registry}{nif9:0>9}{year}1.ZIP"),
        huella: taxpain_core::base64::encode(&Sha256::digest(&zip)),
        claves: claves.len(),
        zip,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatos() {
        assert_eq!(amount(Money::from_cents(123_456)), "1234.56");
        assert_eq!(amount(Money::from_cents(-5)), "-0.05");
        assert_eq!(
            dd_mm_aaaa(time::macros::date!(2025 - 12 - 31)),
            "31.12.2025"
        );
        let d = documents_code(ModelKind::Pymes, true);
        assert_eq!(d.len(), 24);
        assert_eq!(d, "333001001001E00010000000");
        assert_eq!(&documents_code(ModelKind::Normal, false)[..3], "110");
    }
}
