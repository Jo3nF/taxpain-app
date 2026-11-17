//! Banco de pruebas VERI*FACTU (entorno de pruebas de la AEAT): empresa aparte, batería de
//! facturas, envío (el de siempre), consulta y comparación, e informe.

use taxpain_aeat::AeatClient;
use taxpain_aeat::verifactu::{Destino, consultar};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::{invoicing, verifactu_lab as lab};
use taxpain_core::invoice::{Party, PartyId};
use taxpain_vault::{CertificateInfo, Vault};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

fn cert(v: &Vault, fingerprint: Option<&str>) -> CmdResult<Option<CertificateInfo>> {
    let list = v.certificates()?;
    Ok(match fingerprint {
        Some(f) => list.into_iter().find(|c| c.fingerprint_sha256 == f),
        None => list.into_iter().next(),
    })
}

fn local_now() -> (OffsetDateTime, u8, time::Weekday) {
    let now = OffsetDateTime::now_utc();
    let local = now.to_offset(taxpain_core::spain_time::peninsular_offset(now));
    (now, local.hour(), local.weekday())
}

#[derive(serde::Serialize)]
pub struct LabStatus {
    pub is_lab: bool,
    pub info: Option<lab::LabInfo>,
    pub checks: Vec<lab::Check>,
    pub records: Vec<lab::LocalRecord>,
}

#[tauri::command]
pub fn verifactu_lab_status(
    state: tauri::State<'_, AppState>,
    fingerprint: Option<String>,
) -> CmdResult<LabStatus> {
    let v = state.vault()?;
    let (now, hour, weekday) = local_now();
    let c = cert(&v, fingerprint.as_deref())?;
    let info = lab::info(&v)?;
    Ok(LabStatus {
        is_lab: info.is_some(),
        checks: lab::preflight(&v, c.as_ref(), now, hour, weekday)?,
        info,
        records: lab::records(&v)?,
    })
}

/// Crea la empresa «Pruebas VERI*FACTU» con el emisor de la empresa abierta, la deja en modo
/// pruebas y cambia a ella (la interfaz se recarga después).
#[tauri::command]
pub fn verifactu_lab_create(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let (issuer, info) = {
        let v = state.vault()?;
        if lab::info(&v)?.is_some() {
            return Err(CommandError::from("ya estás en la empresa de pruebas"));
        }
        let info = companies::info(&v)?;
        let issuer = invoicing::settings(&v)?.map(|s| s.issuer).unwrap_or(Party {
            id: PartyId::Nif(info.nif.clone().unwrap_or_default()),
            name: info.name.clone(),
            address: None,
            person: None,
        });
        (issuer, info)
    };
    let company = state.create_company(
        &CompanyInfo {
            name: format!("Pruebas VERI*FACTU ({})", info.name),
            nif: info.nif,
        },
        None,
    )?;
    let now = OffsetDateTime::now_utc();
    {
        let v = state.open_company(&company.id)?;
        invoicing::save_settings(&v, &lab::settings_for(issuer), now)?;
        lab::mark(&v, now)?;
    }
    state.switch_company(&company.id)
}

/// Pone como emisor de pruebas el titular del certificado (o la entidad que representa).
#[tauri::command]
pub fn verifactu_lab_use_cert_nif(
    state: tauri::State<'_, AppState>,
    fingerprint: Option<String>,
) -> CmdResult<()> {
    let v = state.vault()?;
    if lab::info(&v)?.is_none() {
        return Err(CommandError::from("sólo en la empresa de pruebas"));
    }
    let c = cert(&v, fingerprint.as_deref())?
        .ok_or_else(|| CommandError::from("importa antes el certificado"))?;
    let (nif, name) = match (&c.entity_nif, &c.entity_name) {
        (Some(n), Some(name)) => (n.clone(), name.clone()),
        _ => (
            c.holder_nif.clone(),
            c.holder_name.clone().unwrap_or_default(),
        ),
    };
    let mut s =
        invoicing::settings(&v)?.ok_or_else(|| CommandError::from("falta la facturación"))?;
    s.issuer = Party {
        id: PartyId::Nif(nif),
        name,
        address: s.issuer.address.clone(),
        person: None,
    };
    invoicing::save_settings(&v, &s, OffsetDateTime::now_utc())?;
    Ok(())
}

#[tauri::command]
pub fn verifactu_lab_generate(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    let now = OffsetDateTime::now_utc();
    Ok(lab::generate(
        &*state.vault()?,
        taxpain_core::spain_time::peninsular_date(now),
        now,
    )?)
}

#[tauri::command]
pub fn verifactu_lab_cancel(state: tauri::State<'_, AppState>) -> CmdResult<String> {
    Ok(lab::cancel_one(
        &*state.vault()?,
        OffsetDateTime::now_utc(),
    )?)
}

/// Consulta en el entorno de pruebas los registros del mes (con el certificado y Touch ID) y
/// los compara con los de TaxPain. Sólo lectura.
#[tauri::command]
pub async fn verifactu_lab_query(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
    year: i32,
    month: u8,
) -> CmdResult<Vec<lab::Compared>> {
    let (request, nombre, nif) = {
        let v = state.vault()?;
        if lab::info(&v)?.is_none() {
            return Err(CommandError::from("sólo en la empresa de pruebas"));
        }
        let s =
            invoicing::settings(&v)?.ok_or_else(|| CommandError::from("falta la facturación"))?;
        let PartyId::Nif(nif) = s.issuer.id else {
            return Err(CommandError::from("el emisor debe tener NIF"));
        };
        let request = v.prepare_identity(
            &fingerprint,
            "Consultar en la AEAT (pruebas) los registros VERI*FACTU",
            OffsetDateTime::now_utc(),
        )?;
        (request, s.issuer.name, nif)
    };
    let identity = state.identity(request)?;
    let client = AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::PRUEBAS)?;
    let mut regs = Vec::new();
    let mut page: Option<taxpain_invoicing::verifactu::ConsultaRegistro> = None;
    for _ in 0..20 {
        let body =
            taxpain_invoicing::verifactu::consulta(&nombre, &nif, year, month, None, page.as_ref());
        let r = consultar(&client, &Destino::pruebas(), &body).await?;
        let more = r.mas;
        page = r.registros.last().cloned();
        regs.extend(r.registros);
        if !more || page.is_none() {
            break;
        }
    }
    let v = state.vault()?;
    let c = lab::compare(&v, &regs, year, month)?;
    lab::save_query(&v, &c)?;
    Ok(c)
}

#[tauri::command]
pub fn verifactu_lab_report_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    fingerprint: Option<String>,
    path: String,
) -> CmdResult<()> {
    let path = crate::salida::salida_permitida(&app, &path, &["md", "txt"])?;
    let v = state.vault()?;
    let (now, hour, weekday) = local_now();
    let c = cert(&v, fingerprint.as_deref())?;
    let checks = lab::preflight(&v, c.as_ref(), now, hour, weekday)?;
    crate::salida::escribir(&path, lab::report_markdown(&v, &checks, now)?)?;
    Ok(())
}
