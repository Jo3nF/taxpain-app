//! Pruebas de VERI*FACTU en el **entorno de pruebas** de la AEAT, en una empresa aparte para no
//! gastar la numeración real:
//!
//! 1. comprobaciones previas ([`preflight`]): empresa de pruebas, facturación en modo pruebas,
//!    certificado vigente y del mismo NIF que el emisor;
//! 2. una batería de facturas ([`generate`]): completa al 21 %, con dos tipos y retención,
//!    simplificada y rectificativa por diferencias; luego una anulación ([`cancel_one`]);
//! 3. envío (el de siempre, con confirmación y Touch ID) y **consulta** a la AEAT de lo que ha
//!    registrado, comparando estado y huella con lo que generó TaxPain ([`compare`]);
//! 4. un informe en Markdown ([`report_markdown`]) para revisar los resultados.
//!
//! Nada de esto puede ir a producción: la empresa de pruebas tiene VERI*FACTU en modo pruebas y
//! el envío sólo conoce el entorno de pruebas en desarrollo.

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{
    InvoiceKind, InvoiceLine, Party, PartyId, Rectification, RectificationMethod, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::verifactu::ConsultaRegistro;
use taxpain_vault::{CertificateInfo, Vault};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::{self, InvoiceDraft};

/// Marca de la empresa de pruebas (en sus ajustes).
pub const LAB_KEY: &str = "verifactu_lab";
const LAST_QUERY_KEY: &str = "verifactu_lab_consulta";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabInfo {
    pub created_at: String,
    /// Facturas de la batería (por número), en el orden en que se generaron.
    #[serde(default)]
    pub invoices: Vec<String>,
}

pub fn info(v: &Vault) -> Result<Option<LabInfo>> {
    Ok(v.setting(LAB_KEY)?)
}

/// Marca la empresa abierta como empresa de pruebas.
pub fn mark(v: &Vault, now: OffsetDateTime) -> Result<()> {
    if info(v)?.is_none() {
        v.set_setting(
            LAB_KEY,
            &LabInfo {
                created_at: invoicing::now_str(now),
                invoices: vec![],
            },
        )?;
    }
    Ok(())
}

/// Facturación de la empresa de pruebas: el emisor dado, series propias y VERI*FACTU en modo
/// pruebas.
pub fn settings_for(issuer: Party) -> crate::settings::InvoicingSettings {
    use crate::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
    InvoicingSettings {
        issuer,
        default_series: "PRU".into(),
        rectifying_series: "PRUR".into(),
        simplified_series: "PRUS".into(),
        simplified_rectifying_series: "PRURS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: true,
            facturae: false,
            ubl: false,
        },
        destination: None,
        rounding: taxpain_core::invoice::RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: true,
            installation_id: format!("pruebas-{}", uuid_like()),
            production: false,
        },
        footer: Some(
            "Factura de PRUEBA para el entorno de pruebas de la AEAT. Sin validez.".into(),
        ),
        activity: None,
        simplified_up_to_3000: false,
    }
}

fn uuid_like() -> String {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap_or_default();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Una comprobación previa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
    /// Si falla, no se puede enviar.
    pub blocking: bool,
}

fn check(id: &str, label: &str, ok: bool, detail: impl Into<String>, blocking: bool) -> Check {
    Check {
        id: id.into(),
        label: label.into(),
        ok,
        detail: detail.into(),
        blocking,
    }
}

/// NIF del emisor de la facturación.
fn issuer_nif(v: &Vault) -> Result<Option<(String, String)>> {
    Ok(invoicing::settings(v)?.and_then(|s| match s.issuer.id {
        PartyId::Nif(n) => Some((n, s.issuer.name)),
        PartyId::Foreign { .. } => None,
    }))
}

/// Comprobaciones antes de enviar. `local` es la hora de España (para el horario del entorno).
pub fn preflight(
    v: &Vault,
    cert: Option<&CertificateInfo>,
    now: OffsetDateTime,
    local_hour: u8,
    weekday: time::Weekday,
) -> Result<Vec<Check>> {
    let mut out = Vec::new();
    let lab = info(v)?.is_some();
    out.push(check(
        "empresa",
        "Empresa de pruebas",
        lab,
        if lab {
            "Las facturas de prueba no tocan la numeración de tus empresas reales."
        } else {
            "Crea la empresa de pruebas: así las facturas de prueba no gastan tu numeración real."
        },
        true,
    ));
    let s = invoicing::settings(v)?;
    let mode_ok = s
        .as_ref()
        .is_some_and(|s| s.verifactu.enabled && !s.verifactu.production);
    out.push(check(
        "modo",
        "VERI*FACTU en modo pruebas",
        mode_ok,
        if mode_ok {
            "Los registros van al entorno de pruebas (prewww1.aeat.es) y el QR apunta a la web de pruebas."
        } else {
            "La facturación de esta empresa no está en modo pruebas."
        },
        true,
    ));
    match cert {
        None => out.push(check(
            "certificado",
            "Certificado electrónico",
            false,
            "Importa tu certificado en esta empresa (Ajustes → Certificado electrónico).",
            true,
        )),
        Some(c) => {
            let until =
                OffsetDateTime::parse(&c.valid_to, &time::format_description::well_known::Rfc3339)
                    .ok();
            let valid = until.is_some_and(|u| u > now);
            let day = c.valid_to.get(..10).unwrap_or(&c.valid_to);
            out.push(check(
                "certificado",
                "Certificado electrónico",
                valid,
                if valid {
                    format!(
                        "{} · caduca el {day}",
                        c.entity_name
                            .clone()
                            .or(c.holder_name.clone())
                            .unwrap_or_default()
                    )
                } else {
                    format!("Caducado o no legible (válido hasta {day})")
                },
                true,
            ));
            let cert_nif = c.entity_nif.clone().unwrap_or(c.holder_nif.clone());
            let issuer = issuer_nif(v)?;
            let same = issuer.as_ref().is_some_and(|(n, _)| *n == cert_nif);
            out.push(check(
                "nif",
                "El certificado es del emisor",
                same,
                match &issuer {
                    Some((n, _)) if same => format!("Emisor y certificado: {n}"),
                    Some((n, _)) => format!(
                        "El emisor es {n} y el certificado es de {cert_nif}: usa el NIF del certificado como emisor de pruebas (botón «Usar el NIF del certificado»)."
                    ),
                    None => "Falta configurar el emisor.".into(),
                },
                true,
            ));
        }
    }
    let open = !matches!(weekday, time::Weekday::Saturday | time::Weekday::Sunday)
        && (8..15).contains(&local_hour);
    out.push(check(
        "horario",
        "Horario del entorno de pruebas",
        open,
        if open {
            "Dentro del horario habitual (laborables de 8 a 15 h)."
        } else {
            "Fuera del horario habitual (laborables de 8 a 15 h): puede no responder. TODO(VERIFY) con la AEAT."
        },
        false,
    ));
    let pending: i64 = v.connection().query_row(
        "SELECT COUNT(*) FROM verifactu_record WHERE send_status = 'pendiente'",
        [],
        |r| r.get(0),
    )?;
    out.push(check(
        "pendientes",
        "Registros pendientes de enviar",
        true,
        format!("{pending}"),
        false,
    ));
    Ok(out)
}

/// Cliente de las facturas de prueba: un NIF real que está en el censo (la AEAT), para que el
/// entorno no rechace el destinatario. `TODO(VERIFY)`: si el entorno de pruebas valida el censo.
fn test_client() -> Party {
    Party {
        id: PartyId::Nif("Q2826000H".into()),
        name: "AGENCIA ESTATAL DE ADMINISTRACION TRIBUTARIA".into(),
        // Domicilio obligatorio en la factura completa (art. 6.1.e RD 1619/2012).
        address: Some(taxpain_core::invoice::Address {
            street: "Calle Infanta Mercedes 37".into(),
            post_code: "28020".into(),
            town: "Madrid".into(),
            province: "Madrid".into(),
            country: "ES".into(),
        }),
        person: None,
    }
}

fn line(desc: &str, euros: i64, vat: i32, withholding: Option<i32>) -> InvoiceLine {
    InvoiceLine {
        description: desc.into(),
        quantity_milli: 1000,
        unit_price: Money::from_euros(euros, 0),
        discount: Rate::ZERO,
        vat: VatTreatment::Taxed {
            rate: Rate::percent(vat),
        },
        surcharge: None,
        withholding: withholding.map(Rate::percent),
        advance: false,
    }
}

/// Emite la batería de facturas de prueba. Sólo en la empresa de pruebas.
pub fn generate(v: &Vault, today: Date, now: OffsetDateTime) -> Result<Vec<String>> {
    let mut lab = info(v)?.ok_or_else(|| {
        AppError::Other("esto sólo se hace en la empresa de pruebas VERI*FACTU".into())
    })?;
    let mut numbers = Vec::new();
    let emit = |d: InvoiceDraft| -> Result<String> {
        let r = invoicing::issue(v, &d, now, true)?;
        Ok(r.number)
    };
    let draft = |kind, recipient, lines, desc: &str, rectified: Vec<(String, Date)>| InvoiceDraft {
        kind,
        series: None,
        issue_date: today,
        operation_date: None,
        recipient,
        lines,
        rectified,
        rectified_amounts: None,
        description: desc.into(),
        notes: Some("Factura de prueba".into()),
        currency: None,
        oss: None,
    };
    let f1 = emit(draft(
        InvoiceKind::Completa,
        Some(test_client()),
        vec![line("Prueba VERI*FACTU: servicio al 21 %", 100, 21, None)],
        "Prueba 1: factura completa al 21 %",
        vec![],
    ))?;
    numbers.push(f1.clone());
    numbers.push(emit(draft(
        InvoiceKind::Completa,
        Some(test_client()),
        vec![
            line("Prueba VERI*FACTU: servicio profesional", 200, 21, Some(15)),
            line("Prueba VERI*FACTU: producto al 10 %", 50, 10, None),
        ],
        "Prueba 2: dos tipos de IVA y retención de IRPF",
        vec![],
    ))?);
    numbers.push(emit(draft(
        InvoiceKind::Simplificada,
        None,
        vec![line("Prueba VERI*FACTU: venta al contado", 20, 21, None)],
        "Prueba 3: factura simplificada",
        vec![],
    ))?);
    numbers.push(emit(draft(
        InvoiceKind::Rectificativa(Rectification {
            code: 1,
            method: RectificationMethod::Diferencias,
        }),
        Some(test_client()),
        vec![line(
            "Prueba VERI*FACTU: rectificación de la prueba 1",
            -10,
            21,
            None,
        )],
        "Prueba 4: rectificativa por diferencias de la prueba 1",
        vec![(f1, today)],
    ))?);
    lab.invoices.extend(numbers.iter().cloned());
    v.set_setting(LAB_KEY, &lab)?;
    Ok(numbers)
}

/// Anula la segunda factura de la batería (genera el registro de anulación).
pub fn cancel_one(v: &Vault, now: OffsetDateTime) -> Result<String> {
    let lab = info(v)?.ok_or_else(|| AppError::Other("no es la empresa de pruebas".into()))?;
    let number = lab
        .invoices
        .get(1)
        .cloned()
        .ok_or_else(|| AppError::Other("genera antes las facturas de prueba".into()))?;
    let id: i64 = v.connection().query_row(
        "SELECT id FROM invoice_issued WHERE number = ?1",
        [&number],
        |r| r.get(0),
    )?;
    invoicing::cancel(v, id, now, true)?;
    Ok(number)
}

/// Lo que dice la AEAT de un registro frente a lo que generó TaxPain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Compared {
    pub num_serie: String,
    pub local_status: String,
    pub aeat_status: Option<String>,
    pub huella_ok: Option<bool>,
    pub error: Option<String>,
    pub ok: bool,
}

/// Compara la consulta de un mes (`year`, `month`: el periodo de imputación consultado, por la
/// fecha de expedición) con los registros locales de ese mes (el último de cada factura). Las
/// facturas de otros meses no se miran: no salen en esa consulta y no es que falten en la AEAT.
pub fn compare(
    v: &Vault,
    regs: &[ConsultaRegistro],
    year: i32,
    month: u8,
) -> Result<Vec<Compared>> {
    let mut stmt = v.connection().prepare(
        "SELECT num_serie, record_type, send_status, huella FROM verifactu_record r
         WHERE id = (SELECT MAX(id) FROM verifactu_record x WHERE x.num_serie = r.num_serie)
           AND substr(fecha, 4, 7) = ?1
         ORDER BY id",
    )?;
    let rows = stmt.query_map([format!("{month:02}-{year:04}")], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (num, kind, status, huella) = row?;
        let aeat = regs.iter().find(|g| g.num_serie == num);
        let aeat_status = aeat.map(|g| g.estado.clone());
        // La huella que devuelve la consulta es la del alta; tras una anulación el estado es
        // «Anulado».
        let huella_ok = aeat
            .and_then(|g| g.huella.as_ref())
            .filter(|_| kind == "alta")
            .map(|h| h.eq_ignore_ascii_case(&huella));
        let expected = if kind == "anulacion" {
            "Anulado"
        } else {
            "Correcto"
        };
        let ok = aeat_status.as_deref() == Some(expected) && huella_ok != Some(false);
        out.push(Compared {
            num_serie: num,
            local_status: format!("{kind}: {status}"),
            aeat_status,
            huella_ok,
            error: aeat.and_then(|g| {
                g.codigo_error
                    .as_ref()
                    .map(|c| format!("{c} {}", g.descripcion_error.clone().unwrap_or_default()))
            }),
            ok,
        });
    }
    Ok(out)
}

/// Guarda la última consulta (para el informe).
pub fn save_query(v: &Vault, c: &[Compared]) -> Result<()> {
    v.set_setting(
        LAST_QUERY_KEY,
        &serde_json::to_value(c).map_err(|e| AppError::Other(e.to_string()))?,
    )?;
    Ok(())
}

/// Un registro local, para el informe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LocalRecord {
    pub num_serie: String,
    pub record_type: String,
    pub send_status: String,
    pub error: Option<String>,
    pub csv: Option<String>,
    pub sent_at: Option<String>,
}

pub fn records(v: &Vault) -> Result<Vec<LocalRecord>> {
    let mut stmt = v.connection().prepare(
        "SELECT num_serie, record_type, send_status, error_code, error_text, csv, sent_at
         FROM verifactu_record ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        let code: Option<String> = r.get(3)?;
        let text: Option<String> = r.get(4)?;
        Ok(LocalRecord {
            num_serie: r.get(0)?,
            record_type: r.get(1)?,
            send_status: r.get(2)?,
            error: code.map(|c| format!("{c} {}", text.unwrap_or_default())),
            csv: r.get(5)?,
            sent_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Informe en Markdown (sin datos de clientes: sólo números de factura, estados y códigos).
pub fn report_markdown(v: &Vault, checks: &[Check], now: OffsetDateTime) -> Result<String> {
    let mut s = String::new();
    s.push_str("# Pruebas VERI*FACTU (entorno de pruebas de la AEAT)\n\n");
    s.push_str(&format!(
        "- Fecha: {}\n- TaxPain {}\n\n## Comprobaciones previas\n\n",
        invoicing::now_str(now),
        env!("CARGO_PKG_VERSION")
    ));
    for c in checks {
        s.push_str(&format!(
            "- {} {}: {}\n",
            if c.ok { "✅" } else { "❌" },
            c.label,
            c.detail
        ));
    }
    s.push_str("\n## Registros enviados\n\n| Factura | Registro | Estado | Error | CSV |\n|---|---|---|---|---|\n");
    for r in records(v)? {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            r.num_serie,
            r.record_type,
            r.send_status,
            r.error.unwrap_or_default(),
            r.csv.unwrap_or_default()
        ));
    }
    let last: Option<Vec<serde_json::Value>> = v.setting(LAST_QUERY_KEY)?;
    s.push_str("\n## Consulta a la AEAT\n\n");
    match last {
        None => s.push_str("Sin consultar todavía.\n"),
        Some(rows) => {
            s.push_str("| Factura | TaxPain | AEAT | Huella | Error |\n|---|---|---|---|---|\n");
            for r in rows {
                let g = |k: &str| {
                    r.get(k)
                        .map(|x| x.to_string().trim_matches('"').to_owned())
                        .unwrap_or_default()
                };
                s.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    g("num_serie"),
                    g("local_status"),
                    g("aeat_status"),
                    match r.get("huella_ok").and_then(serde_json::Value::as_bool) {
                        Some(true) => "coincide",
                        Some(false) => "NO coincide",
                        None => "—",
                    },
                    g("error").replace("null", "")
                ));
            }
        }
    }
    Ok(s)
}
