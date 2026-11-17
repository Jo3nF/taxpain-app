//! Asistente IA (Fase 7), parte de la app: ajustes, clave de API y el resumen de datos que se
//! le da como contexto.
//!
//! El resumen sólo lleva cifras agregadas y estado (nada de nombres de clientes ni del
//! certificado); aun así pasa por el anonimizador de `taxpain-assistant` antes de salir.
//! La clave de API se guarda en la BD cifrada y no vuelve nunca a la interfaz.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use taxpain_assistant::Provider;
use taxpain_core::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;

use crate::error::{AppError, Result};
use crate::{dashboard, invoicing, models};

const SETTINGS_KEY: &str = "assistant";
const API_KEY: &str = "assistant.api_key";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantSettings {
    pub provider: Provider,
    /// El usuario ha aceptado que sus preguntas y un resumen anonimizado de sus cifras se
    /// envíen al proveedor (si es remoto).
    pub consent: bool,
}

/// Ajustes tal como los ve la interfaz (sin la clave).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssistantStatus {
    pub settings: AssistantSettings,
    pub has_api_key: bool,
    pub remote: bool,
}

pub fn status(v: &Vault) -> Result<AssistantStatus> {
    let settings: AssistantSettings = v.setting(SETTINGS_KEY)?.unwrap_or_default();
    Ok(AssistantStatus {
        remote: settings.provider.is_remote(),
        has_api_key: api_key(v)?.is_some(),
        settings,
    })
}

/// Guarda los ajustes. Lo aceptado vale para el host al que se aceptó: si el proveedor nuevo
/// manda los datos a otro sitio (o por primera vez fuera del Mac,
/// [`Provider::needs_new_consent`]), el consentimiento se guarda como no dado aunque llegue
/// aceptado, y hace falta aceptarlo otra vez, ya con ese proveedor guardado. Así un cambio de
/// proveedor nunca hereda lo que se aceptó para otro.
pub fn save_settings(v: &Vault, s: &AssistantSettings, now: OffsetDateTime) -> Result<()> {
    s.provider
        .endpoint()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let previous: AssistantSettings = v.setting(SETTINGS_KEY)?.unwrap_or_default();
    let mut s = s.clone();
    if s.consent && s.provider.needs_new_consent(&previous.provider) {
        s.consent = false;
    }
    let s = &s;
    v.set_setting(SETTINGS_KEY, s)?;
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "assistant.settings",
        s.provider.model(),
        &serde_json::json!({ "remote": s.provider.is_remote(), "consent": s.consent }),
    )?;
    Ok(())
}

/// Con un proveedor externo, el usuario tiene que haber aceptado qué se le envía.
pub fn require_consent(s: &AssistantStatus) -> Result<()> {
    if s.remote && !s.settings.consent {
        return Err(AppError::Other(
            "Activa el asistente en sus ajustes y acepta qué datos se envían al proveedor.".into(),
        ));
    }
    Ok(())
}

/// Nombre del secreto de la clave para el proveedor configurado: va ligada a su host, para que
/// cambiar de proveedor nunca envíe la clave de uno a otro. Los modelos locales no usan clave.
fn key_name(v: &Vault) -> Result<Option<String>> {
    let settings: AssistantSettings = v.setting(SETTINGS_KEY)?.unwrap_or_default();
    if !settings.provider.is_remote() {
        return Ok(None);
    }
    let host = settings
        .provider
        .endpoint()
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .ok_or_else(|| AppError::Other("proveedor del asistente no válido".into()))?;
    Ok(Some(format!("{API_KEY}@{host}")))
}

/// Guarda (o borra, con `None`) la clave de API del proveedor configurado.
pub fn set_api_key(v: &Vault, key: Option<&str>, now: OffsetDateTime) -> Result<()> {
    let Some(name) = key_name(v)? else {
        return Err(AppError::Other(
            "un modelo local no necesita clave de API".into(),
        ));
    };
    let conn = v.connection();
    match key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => {
            conn.execute(
                "INSERT INTO app_secret(name, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![name, k, invoicing::now_str(now)],
            )?;
        }
        None => {
            conn.execute("DELETE FROM app_secret WHERE name = ?1", [&name])?;
        }
    }
    audit::append(
        conn,
        &invoicing::now_str(now),
        Actor::User,
        "assistant.api_key",
        if key.is_some() { "guardada" } else { "borrada" },
        &serde_json::json!({ "proveedor": name.trim_start_matches(API_KEY).trim_start_matches('@') }),
    )?;
    Ok(())
}

/// Clave de API del proveedor configurado (sólo para la petición; nunca para la interfaz ni
/// los registros). `None` con un modelo local.
pub fn api_key(v: &Vault) -> Result<Option<String>> {
    let Some(name) = key_name(v)? else {
        return Ok(None);
    };
    Ok(v.connection()
        .query_row(
            "SELECT value FROM app_secret WHERE name = ?1",
            [name],
            |r| r.get(0),
        )
        .optional()?)
}

fn fecha(d: time::Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn eur(m: Money) -> String {
    m.to_string()
}

/// Resumen de la situación para el asistente a fecha `today`.
pub fn context(v: &Vault, today: time::Date) -> Result<String> {
    Ok(context_with_third_parties(v, today)?.0)
}

/// Como [`context`], con los nombres e identificadores de los terceros que nombran sus avisos
/// (los gastos de viaje del 111, p. ej.): se pasan al anonimizador como valores conocidos, para
/// que vayan con marcadores.
pub fn context_with_third_parties(v: &Vault, today: time::Date) -> Result<(String, Vec<String>)> {
    let (year, month) = (today.year(), u8::from(today.month()));
    let mut out = String::new();
    let mut third_parties = Vec::new();
    let settings = invoicing::settings(v)?;
    let who = match &settings {
        Some(s) if s.issuer.person.is_some() => "autónomo (persona física)",
        Some(_) => "sociedad (persona jurídica)",
        None => "sin configurar todavía",
    };
    let _ = writeln!(out, "- Tipo de contribuyente: {who}.");
    let _ = writeln!(out, "- Fecha: mes {month} de {year}.");
    let d = dashboard::dashboard(v, year, month)?;
    if d.sin_contabilidad {
        // Sin asientos, los ceros no dicen que no haya habido actividad.
        let _ = writeln!(
            out,
            "- TaxPain no tiene la contabilidad de {year} (ningún asiento): no se sabe cuánto ha \
             ingresado ni gastado."
        );
        for c in &d.presentado {
            let _ = writeln!(
                out,
                "- Según lo presentado a la AEAT en {year}: {} {} ({}, {}).",
                c.titulo,
                eur(c.valor),
                c.fuente,
                c.periodos
            );
        }
    } else {
        let _ = writeln!(
            out,
            "- Contabilidad {year} hasta hoy (bases sin IVA): ingresos {}, gastos {}, resultado {}.",
            eur(d.income),
            eur(d.expenses),
            eur(d.profit)
        );
        let _ = writeln!(
            out,
            "- Tesorería {}; pendiente de cobro {}; pendiente de pago {}.",
            eur(d.cash),
            eur(d.receivables),
            eur(d.payables)
        );
    }
    // Sin asientos, los borradores del trimestre saldrían a cero: no se dan.
    let estimates = if d.sin_contabilidad {
        &[][..]
    } else {
        &d.estimates[..]
    };
    for e in estimates {
        if let Some(note) = &e.note {
            let _ = writeln!(
                out,
                "- Modelo {} {}: sin estimar ({note}).",
                e.model, e.period
            );
            continue;
        }
        let _ = writeln!(
            out,
            "- Borrador del modelo {} del {}T: {} ({}).",
            e.model,
            e.quarter,
            eur(e.result.abs()),
            if e.result.cents() >= 0 {
                "a ingresar"
            } else {
                "a compensar o devolver"
            }
        );
        if let Ok(draft) = models::draft(v, &e.model, year, e.quarter) {
            for w in draft.warnings.iter().take(5) {
                let _ = writeln!(out, "  - Aviso del {}: {w}", e.model);
            }
            third_parties.extend(draft.terceros);
        }
    }
    // Próximos plazos según el perfil fiscal (si está configurado).
    let profile: Option<taxpain_core::calendar::TaxpayerProfile> = v.setting("taxpayer_profile")?;
    if let Some(profile) = profile {
        let mut next = Vec::new();
        for y in [year, year + 1] {
            let (h, _) =
                taxpain_core::calendar::calendar_for(y, profile.ccaa, &profile.festivos_locales);
            next.extend(
                taxpain_core::calendar::deadlines_in_year(&profile, y, &h)
                    .into_iter()
                    .filter(|d| d.due_on >= today),
            );
        }
        next.sort_by_key(|d| d.due_on);
        for d in next.iter().take(5) {
            let _ = writeln!(
                out,
                "- Plazo: modelo {} ({}) {} {}, del {} al {}.",
                d.model.code(),
                d.model.description(),
                d.period.code(),
                d.fiscal_year,
                fecha(d.starts_on),
                fecha(d.due_on)
            );
        }
    }
    let p = &d.pending;
    let _ = writeln!(
        out,
        "- Pendiente: {} recibidas por revisar, {} movimientos del banco sin conciliar, {} notificaciones de la AEAT sin abrir, {} registros VERI*FACTU sin enviar.",
        p.received_to_review, p.bank_unreconciled, p.notices_pending, p.verifactu_unsent
    );
    let filed = models::filed_returns(v, Some(year))?;
    if !filed.is_empty() {
        let list: Vec<String> = filed
            .iter()
            .map(|f| format!("{} {} ({})", f.model, f.period, f.environment))
            .collect();
        let _ = writeln!(
            out,
            "- Presentados desde TaxPain en {year}: {}.",
            list.join(", ")
        );
    }
    Ok((out, third_parties))
}

/// Herramientas de sólo lectura del asistente (`taxpain_assistant::tools::LOCAL_TOOLS`): cada
/// una consulta el almacén y devuelve texto; ninguna escribe ni presenta. La app la llama con el
/// almacén bloqueado sólo durante la consulta.
pub fn tool_call(
    v: &Vault,
    today: time::Date,
    name: &str,
    input: &serde_json::Value,
) -> std::result::Result<String, String> {
    let int = |k: &str| input.get(k).and_then(serde_json::Value::as_i64);
    let text = |k: &str| {
        input
            .get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    let year = || {
        int("ejercicio")
            .and_then(|y| i32::try_from(y).ok())
            .filter(|y| (2000..=2100).contains(y))
            .ok_or_else(|| "falta el ejercicio (AAAA)".to_string())
    };
    let e = |x: AppError| x.to_string();
    match name {
        "plazos_proximos" => {
            let lines = upcoming_deadlines(v, today).map_err(e)?;
            Ok(if lines.is_empty() {
                "Sin perfil fiscal configurado: no hay plazos calculados.".into()
            } else {
                lines.join("\n")
            })
        }
        "borrador_modelo" => {
            let model = text("modelo");
            let quarter = int("trimestre")
                .and_then(|q| u8::try_from(q).ok())
                .unwrap_or(0);
            let d = models::draft(v, &model, year()?, quarter).map_err(e)?;
            let mut out = format!(
                "Borrador del modelo {} {} ({}): resultado {}.\nCasillas: ",
                d.model,
                d.year,
                models::period_code(&d.model, d.quarter),
                eur(d.result)
            );
            out.push_str(
                &d.boxes
                    .iter()
                    .map(|(b, m)| models::casilla(&d.model, *b, *m))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            // Lo que devuelve una herramienta no se puede tapar con marcadores conocidos: sin los
            // nombres ni los NIF de los terceros que nombran los avisos.
            for w in d.warnings.iter().take(8) {
                out.push_str(&format!(
                    "\nAviso: {}",
                    models::sin_terceros(w, &d.terceros)
                ));
            }
            Ok(out)
        }
        "buscar_facturas" => {
            let y = year()?;
            let needle = text("texto").to_lowercase();
            let hit = |s: &str| needle.is_empty() || s.to_lowercase().contains(&needle);
            let rows: Vec<String> = if text("tipo") == "recibidas" {
                crate::received::list_received(v, Some("revisada"))
                    .map_err(e)?
                    .into_iter()
                    .filter(|r| {
                        r.issue_date
                            .as_deref()
                            .is_some_and(|d| d.starts_with(&y.to_string()))
                    })
                    .filter(|r| {
                        hit(r.number.as_deref().unwrap_or(""))
                            || hit(r.supplier_name.as_deref().unwrap_or(""))
                    })
                    .take(20)
                    .map(|r| {
                        format!(
                            "{} · {} · {} · total {}",
                            r.issue_date.unwrap_or_default(),
                            r.number.unwrap_or_default(),
                            r.supplier_name.unwrap_or_default(),
                            r.total.map(eur).unwrap_or_default()
                        )
                    })
                    .collect()
            } else {
                invoicing::list_issued(v, y)
                    .map_err(e)?
                    .into_iter()
                    .filter(|i| hit(&i.number) || hit(i.recipient_name.as_deref().unwrap_or("")))
                    .take(20)
                    .map(|i| {
                        format!(
                            "{} · {} · {} · base {} · total {} · {}",
                            i.issue_date,
                            i.number,
                            i.recipient_name.unwrap_or_else(|| "simplificada".into()),
                            eur(i.base),
                            eur(i.total),
                            i.status
                        )
                    })
                    .collect()
            };
            Ok(if rows.is_empty() {
                "No hay facturas con ese criterio.".into()
            } else {
                rows.join("\n")
            })
        }
        "saldo_cuenta" => {
            let account = text("cuenta");
            if account.is_empty() || !account.bytes().all(|b| b.is_ascii_digit()) {
                return Err("la cuenta es un número del plan (p. ej. 572 o 4300001)".into());
            }
            let y = year()?;
            let tb = crate::accounting::trial_balance(v, y, None).map_err(e)?;
            let (mut d, mut c) = (Money::ZERO, Money::ZERO);
            for r in tb
                .iter()
                .filter(|r| r.account.as_str().starts_with(&account))
            {
                d += r.debit;
                c += r.credit;
            }
            let name = crate::accounting::account_name(v.connection(), &account)
                .map_err(e)?
                .unwrap_or_default();
            Ok(format!(
                "Cuenta {account} {name} en {y}: debe {}, haber {}, saldo {} ({}).",
                eur(d),
                eur(c),
                eur((d - c).abs()),
                if d >= c { "deudor" } else { "acreedor" }
            ))
        }
        other => Err(format!("herramienta desconocida: {other}")),
    }
}

/// Próximos plazos del perfil fiscal (texto, como mucho cinco).
fn upcoming_deadlines(v: &Vault, today: time::Date) -> Result<Vec<String>> {
    let profile: Option<taxpain_core::calendar::TaxpayerProfile> = v.setting("taxpayer_profile")?;
    let Some(profile) = profile else {
        return Ok(Vec::new());
    };
    let mut next = Vec::new();
    for y in [today.year(), today.year() + 1] {
        let (h, _) =
            taxpain_core::calendar::calendar_for(y, profile.ccaa, &profile.festivos_locales);
        next.extend(
            taxpain_core::calendar::deadlines_in_year(&profile, y, &h)
                .into_iter()
                .filter(|d| d.due_on >= today),
        );
    }
    next.sort_by_key(|d| d.due_on);
    Ok(next
        .iter()
        .take(5)
        .map(|d| {
            format!(
                "Modelo {} ({}) {} {}, del {} al {}.",
                d.model.code(),
                d.model.description(),
                d.period.code(),
                d.fiscal_year,
                fecha(d.starts_on),
                fecha(d.due_on)
            )
        })
        .collect())
}

/// Datos de la propia empresa que nunca deben salir hacia el modelo, aunque aparezcan escritos
/// de otra forma (los sustituye `taxpain_assistant::prepare_with`): razón social o nombre y
/// apellidos, NIF, domicilio, IBAN y cuentas bancarias.
pub fn known_values(v: &Vault) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if let Some(s) = invoicing::settings(v)? {
        let p = &s.issuer;
        out.push(p.name.clone());
        match &p.id {
            taxpain_core::invoice::PartyId::Nif(n) => out.push(n.clone()),
            taxpain_core::invoice::PartyId::Foreign { id, .. } => out.push(id.clone()),
        }
        if let Some(person) = &p.person {
            out.push(person.name.clone());
            out.push(person.first_surname.clone());
            if let Some(s2) = &person.second_surname {
                out.push(s2.clone());
            }
        }
        if let Some(a) = &p.address {
            out.push(a.street.clone());
        }
    }
    // El nombre y el NIF de la ficha de la empresa (también antes de configurar la facturación).
    if let Ok(info) = crate::companies::info(v) {
        out.push(info.name.clone());
        if let Some(n) = info.nif {
            out.push(n);
        }
    }
    if let Some(iban) = models::settings(v)?.iban {
        out.push(iban);
    }
    for b in crate::remesas::banks(v)? {
        out.push(b.iban);
    }
    // Los nombres de sociedad, también sin la forma jurídica («ACME» de «ACME, S.L.»), que es
    // como suelen salir en los documentos.
    let sin_forma: Vec<String> = out.iter().filter_map(|n| sin_forma_juridica(n)).collect();
    out.extend(sin_forma);
    out.retain(|s| !s.trim().is_empty());
    out.sort();
    out.dedup();
    Ok(out)
}

/// El nombre sin su forma jurídica al final (S.L., SL, SLU, S.A., Sociedad Limitada…), si la
/// lleva y lo que queda no es demasiado corto para confundirse con otras palabras.
fn sin_forma_juridica(nombre: &str) -> Option<String> {
    static FORMA: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = FORMA.get_or_init(|| {
        regex::Regex::new(
            r"(?i)[,\s]+(?:S\.?\s?L\.?\s?(?:U|P|L)?\.?|S\.?\s?A\.?\s?U?\.?|SOCIEDAD\s+LIMITADA(?:\s+(?:UNIPERSONAL|PROFESIONAL|LABORAL))?|SOCIEDAD\s+AN[ÓO]NIMA(?:\s+UNIPERSONAL)?)\s*$",
        )
        .expect("patrón fijo válido")
    });
    let base = re.replace(nombre.trim(), "").trim().to_owned();
    (base != nombre.trim() && base.chars().count() >= 4).then_some(base)
}

/// Deja constancia de una consulta (sin su contenido).
pub fn record_chat(
    v: &Vault,
    provider: &Provider,
    replaced: usize,
    now: OffsetDateTime,
) -> Result<()> {
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "assistant.chat",
        provider.model(),
        &serde_json::json!({ "remote": provider.is_remote(), "anonymized": replaced }),
    )?;
    Ok(())
}

/// Anota en la auditoría que un fichero entero (PDF o imagen, sin anonimizar) ha salido hacia
/// el proveedor de IA («Full IA»), con su huella: ante una duda o una inspección se sabe qué
/// documentos se enviaron (RGPD arts. 13, 14 y 28).
pub fn record_file(
    v: &Vault,
    provider: &Provider,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "assistant.file",
        provider.model(),
        &serde_json::json!({
            "remote": provider.is_remote(),
            "sha256": hex::encode(Sha256::digest(bytes)),
            "size": bytes.len(),
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::datetime;

    use super::*;

    const AHORA: OffsetDateTime = datetime!(2026-11-12 10:00 UTC);

    fn guarda(v: &Vault, provider: Provider, consent: bool) -> bool {
        save_settings(v, &AssistantSettings { provider, consent }, AHORA).unwrap();
        status(v).unwrap().settings.consent
    }

    fn otro(base_url: &str) -> Provider {
        Provider::OpenAiCompatible {
            base_url: base_url.into(),
            model: "modelo-de-prueba".into(),
        }
    }

    #[test]
    fn cambiar_de_host_pide_aceptar_otra_vez() {
        let v = Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap();
        // Lo primero: el proveedor de siempre, aceptado.
        assert!(guarda(&v, Provider::default(), true));
        // Otro modelo en el mismo host: sigue aceptado.
        assert!(guarda(
            &v,
            Provider::Anthropic {
                model: "otro-modelo".into()
            },
            true
        ));
        // Otro host, aunque llegue aceptado: se guarda sin aceptar…
        assert!(!guarda(&v, otro("https://api.mistral.ai/v1"), true));
        // …y, ya guardado ese proveedor, aceptarlo otra vez sí vale.
        assert!(guarda(&v, otro("https://api.mistral.ai/v1"), true));
        // Un modelo local no lo necesita; volver de él a uno externo, sí.
        assert!(!guarda(&v, otro("http://localhost:11434/v1"), false));
        assert!(!guarda(&v, otro("https://api.mistral.ai/v1"), true));
        assert!(guarda(&v, otro("https://api.mistral.ai/v1"), true));
        // Retirar el consentimiento siempre se guarda.
        assert!(!guarda(&v, otro("https://api.mistral.ai/v1"), false));
    }

    #[test]
    fn nombres_sin_forma_juridica() {
        assert_eq!(sin_forma_juridica("ACME, S.L.").as_deref(), Some("ACME"));
        assert_eq!(
            sin_forma_juridica("EMPRESA DEMO SL").as_deref(),
            Some("EMPRESA DEMO")
        );
        assert_eq!(
            sin_forma_juridica("Estudio Demo Sociedad Limitada").as_deref(),
            Some("Estudio Demo")
        );
        assert_eq!(sin_forma_juridica("TAXI S.A.U.").as_deref(), Some("TAXI"));
        // Sin forma jurídica o demasiado corto: nada.
        assert_eq!(sin_forma_juridica("Carmen Española"), None);
        assert_eq!(sin_forma_juridica("AB SL"), None);
    }
}
