//! Cola de remisión VERI*FACTU: qué se envía, cuándo y qué pasa si falla.
//!
//! - **Control de flujo** (Orden HAC/1177/2024 art. 16.2; [SW] 6.4.4.1): tras cada respuesta no
//!   se vuelve a enviar hasta que pasen los `TiempoEsperaEnvio` segundos que dice la AEAT (60 al
//!   empezar), salvo que se junten 1.000 registros, lo que ocurra antes.
//! - **Reintentos** (art. 16.4): si el envío no llega (sin red, sin respuesta, `SOAP Fault`
//!   `Server`), se reintenta con espera creciente y, como mucho, cada hora; el siguiente envío
//!   lleva `Incidencia = S` y se avisa de cuántos registros quedan sin remitir.
//! - **Rechazo del mensaje** (`SOAP Fault` `Client`, [SW] 5.1): no se reintenta solo, porque se
//!   rechazaría igual; queda bloqueada hasta que el usuario lo pida.
//! - **Respuesta por registro**: ver [`crate::invoicing::apply_submission_response`] (duplicados
//!   como remitidos, registros sin línea reenviados una vez).
//!
//! Aquí no hay red: la orden de Tauri pide el lote ([`siguiente_lote`]), lo envía con el
//! certificado y anota la respuesta ([`registrar_respuesta`]) o el fallo ([`registrar_fallo`]).
//! Todo va al entorno de **pruebas** mientras el interruptor de producción
//! ([`taxpain_core::productor::PRODUCCION_HABILITADA`]) esté apagado.

use serde::Serialize;
use taxpain_invoicing::verifactu::{MAX_RECORDS_PER_SUBMISSION, Response};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::OffsetDateTime;

use crate::Result;
use crate::invoicing::{self, Entorno};
use crate::settings::InvoicingSettings;

/// Veces que la AEAT puede contestar sin una línea para un registro antes de sacarlo de la
/// cola: se reenvía una vez y, a la segunda, queda «sin respuesta» para consultarlo.
pub const MAX_SIN_LINEA: i64 = 2;
/// `TiempoEsperaEnvio` inicial ([SW] 6.4.4.1).
pub const ESPERA_INICIAL: i64 = 60;
/// Tope de la espera entre reintentos tras un fallo: «al menos cada hora» (art. 16.4).
pub const REINTENTO_MAXIMO: i64 = 3600;

/// Si los registros se remiten de verdad a la AEAT (entorno de producción). Sólo con el
/// interruptor de compilación encendido, los datos del productor completos y la empresa con
/// `verifactu.production`. Hoy siempre `false`.
#[must_use]
pub fn remite_a_produccion(s: &InvoicingSettings) -> bool {
    motivo_sin_produccion(s).is_none()
}

/// Por qué no se remite a producción (`None` si se remite).
#[must_use]
pub fn motivo_sin_produccion(s: &InvoicingSettings) -> Option<String> {
    use taxpain_core::productor;
    if !s.verifactu.enabled {
        return Some("VERI*FACTU está desactivado en esta empresa".into());
    }
    if !productor::PRODUCCION_HABILITADA {
        return Some(
            "esta versión de TaxPain sólo envía al entorno de pruebas de la AEAT (el envío a \
             producción está cerrado hasta completar la prueba con certificado real)"
                .into(),
        );
    }
    let faltan = productor::faltan_datos();
    if !faltan.is_empty() {
        return Some(format!(
            "faltan datos del productor para la declaración responsable: {}",
            faltan.join(", ")
        ));
    }
    if !s.verifactu.production {
        return Some("el envío a producción no está activado en esta empresa".into());
    }
    None
}

/// Entorno al que va el siguiente envío.
#[must_use]
pub fn entorno(s: &InvoicingSettings) -> Entorno {
    if remite_a_produccion(s) {
        Entorno::Produccion
    } else {
        Entorno::Pruebas
    }
}

/// Estado de la cola para la interfaz.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstadoCola {
    /// `pruebas` o `produccion`.
    pub entorno: &'static str,
    /// Por qué no se remite a producción (si no se remite).
    pub sin_produccion: Option<String>,
    /// Registros que esperan envío en el entorno actual.
    pub pendientes: i64,
    /// La AEAT contestó dos veces sin línea para ellos: hay que consultarlos.
    pub sin_respuesta: i64,
    /// Rechazados o aceptados con errores, sin subsanar.
    pub rechazados: i64,
    pub aceptados_con_errores: i64,
    /// Aceptados sólo en el entorno de pruebas (no remitidos).
    pub aceptados_pruebas: i64,
    /// Remitidos a producción y aceptados.
    pub remitidos: i64,
    /// Segundos que faltan para poder enviar (0 = ya).
    pub espera_segundos: i64,
    /// Hora del próximo envío permitido (RFC 3339, UTC), si hay que esperar.
    pub proximo_envio: Option<String>,
    /// Último `TiempoEsperaEnvio` de la AEAT.
    pub tiempo_espera: i64,
    /// Desde cuándo hay una incidencia técnica sin resolver (RFC 3339): el siguiente envío
    /// lleva `Incidencia = S`.
    pub incidencia_desde: Option<String>,
    pub fallos_seguidos: i64,
    pub ultimo_intento: Option<String>,
    pub ultimo_resultado: Option<String>,
    pub ultimo_error: Option<String>,
    /// La AEAT rechazó el mensaje entero: no se reintenta solo.
    pub bloqueada: bool,
    /// Aviso para el usuario (art. 16.4: cuántos registros quedan sin remitir).
    pub aviso: Option<String>,
    /// Con VERI*FACTU desactivado: por qué así no es un sistema conforme y desde cuándo.
    pub aviso_modo: Option<String>,
}

#[derive(Debug, Default)]
struct Fila {
    proximo_envio: Option<i64>,
    tiempo_espera: i64,
    incidencia_desde: Option<i64>,
    fallos_seguidos: i64,
    ultimo_intento: Option<i64>,
    ultimo_resultado: Option<String>,
    ultimo_error: Option<String>,
    bloqueada: bool,
}

fn fila(v: &Vault) -> Result<Fila> {
    use taxpain_vault::rusqlite::OptionalExtension;
    let f = v
        .connection()
        .query_row(
            "SELECT proximo_envio, tiempo_espera, incidencia_desde, fallos_seguidos,
                    ultimo_intento, ultimo_resultado, ultimo_error, bloqueada
             FROM verifactu_cola WHERE id = 1",
            [],
            |r| {
                Ok(Fila {
                    proximo_envio: r.get(0)?,
                    tiempo_espera: r.get(1)?,
                    incidencia_desde: r.get(2)?,
                    fallos_seguidos: r.get(3)?,
                    ultimo_intento: r.get(4)?,
                    ultimo_resultado: r.get(5)?,
                    ultimo_error: r.get(6)?,
                    bloqueada: r.get(7)?,
                })
            },
        )
        .optional()?;
    Ok(f.unwrap_or(Fila {
        tiempo_espera: ESPERA_INICIAL,
        ..Fila::default()
    }))
}

fn rfc3339(secs: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(secs)
        .ok()
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
}

fn count(v: &Vault, sql: &str) -> Result<i64> {
    Ok(v.connection()
        .query_row(sql, [MAX_SIN_LINEA], |r| r.get(0))?)
}

/// Registros en cola para `entorno` (los que [`invoicing::pending_submission`] enviaría).
fn pendientes(v: &Vault, entorno: Entorno) -> Result<i64> {
    let statuses = match entorno {
        Entorno::Pruebas => "('pendiente')",
        Entorno::Produccion => "('pendiente', 'pruebas')",
    };
    count(
        v,
        &format!(
            "SELECT count(*) FROM verifactu_record WHERE send_status IN {statuses} AND sin_linea < ?1"
        ),
    )
}

/// Estado de la cola a la hora `now`.
pub fn estado(v: &Vault, now: OffsetDateTime) -> Result<EstadoCola> {
    let s = invoicing::settings(v)?;
    let ent = s.as_ref().map_or(Entorno::Pruebas, entorno);
    let f = fila(v)?;
    let pend = pendientes(v, ent)?;
    let sin_respuesta = count(
        v,
        "SELECT count(*) FROM verifactu_record WHERE send_status = 'pendiente' AND sin_linea >= ?1",
    )?;
    let issues = invoicing::verifactu_issues(v)?;
    let rechazados = issues
        .iter()
        .filter(|i| i.send_status == "incorrecto")
        .count();
    let con_errores = issues.len() - rechazados;
    let por_estado = |st: &str| -> Result<i64> {
        Ok(v.connection().query_row(
            "SELECT count(*) FROM verifactu_record WHERE send_status = ?1",
            [st],
            |r| r.get(0),
        )?)
    };
    let ahora = now.unix_timestamp();
    let espera = if pend >= MAX_RECORDS_PER_SUBMISSION as i64 {
        0
    } else {
        f.proximo_envio.map_or(0, |p| (p - ahora).max(0))
    };
    let sin_remitir = pend + sin_respuesta;
    let aviso = if f.bloqueada {
        Some(format!(
            "La AEAT rechazó el último envío entero: {}. Corrige el problema y pulsa «Reintentar».",
            f.ultimo_error.clone().unwrap_or_default()
        ))
    } else if f.incidencia_desde.is_some() && sin_remitir > 0 {
        Some(format!(
            "No se ha podido remitir a la AEAT: {} sin enviar. Se reintenta solo (como mucho \
             cada hora) y el envío irá marcado como incidencia.",
            plural(sin_remitir)
        ))
    } else if sin_respuesta > 0 {
        Some(format!(
            "La AEAT no ha contestado por {}: consulta en la Sede si los tiene antes de volver a \
             enviarlos.",
            plural(sin_respuesta)
        ))
    } else {
        None
    };
    Ok(EstadoCola {
        entorno: match ent {
            Entorno::Pruebas => "pruebas",
            Entorno::Produccion => "produccion",
        },
        sin_produccion: s.as_ref().and_then(motivo_sin_produccion),
        pendientes: pend,
        sin_respuesta,
        rechazados: rechazados as i64,
        aceptados_con_errores: con_errores as i64,
        aceptados_pruebas: por_estado("pruebas")?,
        remitidos: por_estado("correcto")?,
        espera_segundos: espera,
        proximo_envio: (espera > 0)
            .then(|| f.proximo_envio.and_then(rfc3339))
            .flatten(),
        tiempo_espera: f.tiempo_espera,
        incidencia_desde: f.incidencia_desde.and_then(rfc3339),
        fallos_seguidos: f.fallos_seguidos,
        ultimo_intento: f.ultimo_intento.and_then(rfc3339),
        ultimo_resultado: f.ultimo_resultado,
        ultimo_error: f.ultimo_error,
        bloqueada: f.bloqueada,
        aviso,
        aviso_modo: s
            .as_ref()
            .and_then(|s| aviso_sin_verifactu(s, taxpain_core::spain_time::peninsular_date(now))),
    })
}

fn plural(n: i64) -> String {
    if n == 1 {
        "1 registro".into()
    } else {
        format!("{n} registros")
    }
}

/// Un envío preparado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lote {
    pub ids: Vec<i64>,
    /// Cuerpo `sfLR:RegFactuSistemaFacturacion`.
    pub body: String,
    /// La cabecera lleva `Incidencia = S`.
    pub incidencia: bool,
    pub entorno: Entorno,
}

/// Qué toca hacer ahora con la cola.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Siguiente {
    /// Nada pendiente.
    Nada,
    /// Hay pendientes, pero hay que esperar estos segundos (control de flujo o reintento).
    Esperar(i64),
    /// La AEAT rechazó el mensaje: no se envía hasta [`reabrir`].
    Bloqueada(String),
    Enviar(Lote),
}

/// El siguiente lote, si toca enviarlo a la hora `now`: respeta el tiempo de espera salvo que
/// se hayan juntado 1.000 registros (art. 16.2).
pub fn siguiente_lote(v: &Vault, now: OffsetDateTime) -> Result<Siguiente> {
    let s = invoicing::require_settings(v)?;
    let ent = entorno(&s);
    let f = fila(v)?;
    let pend = pendientes(v, ent)?;
    if pend == 0 {
        return Ok(Siguiente::Nada);
    }
    if f.bloqueada {
        return Ok(Siguiente::Bloqueada(f.ultimo_error.unwrap_or_default()));
    }
    let espera = f
        .proximo_envio
        .map_or(0, |p| (p - now.unix_timestamp()).max(0));
    // Tras un fallo técnico se espera siempre (no hay que machacar a la AEAT); con el control de
    // flujo, 1.000 registros juntos se envían ya.
    let lleno = pend >= MAX_RECORDS_PER_SUBMISSION as i64 && f.incidencia_desde.is_none();
    if espera > 0 && !lleno {
        return Ok(Siguiente::Esperar(espera));
    }
    let incidencia = f.incidencia_desde.is_some();
    match invoicing::pending_submission_with(v, MAX_RECORDS_PER_SUBMISSION, ent, incidencia)? {
        None => Ok(Siguiente::Nada),
        Some((ids, body)) => Ok(Siguiente::Enviar(Lote {
            ids,
            body,
            incidencia,
            entorno: ent,
        })),
    }
}

/// Anota la respuesta de la AEAT a `lote`: estado de cada registro, el `TiempoEsperaEnvio` y
/// fin de la incidencia, si la había.
pub fn registrar_respuesta(
    v: &Vault,
    lote: &Lote,
    resp: &Response,
    now: OffsetDateTime,
) -> Result<()> {
    invoicing::apply_submission_response(v, &lote.ids, resp, lote.entorno, now)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if lote.incidencia {
        for id in &lote.ids {
            tx.execute(
                "UPDATE verifactu_record SET incidencia = 1 WHERE id = ?1",
                [id],
            )?;
        }
    }
    let espera = i64::from(resp.tiempo_espera_envio);
    let ahora = now.unix_timestamp();
    tx.execute(
        "INSERT INTO verifactu_cola(id, proximo_envio, tiempo_espera, incidencia_desde,
           fallos_seguidos, ultimo_intento, ultimo_resultado, ultimo_error, bloqueada)
         VALUES (1, ?1, ?2, NULL, 0, ?3, ?4, NULL, 0)
         ON CONFLICT(id) DO UPDATE SET proximo_envio = excluded.proximo_envio,
           tiempo_espera = excluded.tiempo_espera, incidencia_desde = NULL, fallos_seguidos = 0,
           ultimo_intento = excluded.ultimo_intento, ultimo_resultado = excluded.ultimo_resultado,
           ultimo_error = NULL, bloqueada = 0",
        params![ahora + espera, espera, ahora, resp.estado_envio],
    )?;
    tx.commit()?;
    Ok(())
}

/// Espera antes del reintento número `fallos` (1, 2…): 60 s, 120 s, 240 s… hasta una hora.
#[must_use]
pub fn espera_reintento(fallos: i64) -> i64 {
    let exp = u32::try_from(fallos.saturating_sub(1).clamp(0, 16)).unwrap_or(16);
    ESPERA_INICIAL
        .saturating_mul(2_i64.saturating_pow(exp))
        .min(REINTENTO_MAXIMO)
}

/// Anota que el envío de `lote` no llegó. `del_mensaje`: la AEAT rechazó el mensaje entero
/// (`SOAP Fault` `Client`), así que no se reintenta solo. Si no, es una incidencia técnica: se
/// reintenta más tarde y con `Incidencia = S`. Los registros siguen pendientes.
pub fn registrar_fallo(
    v: &Vault,
    lote: &Lote,
    error: &str,
    del_mensaje: bool,
    now: OffsetDateTime,
) -> Result<()> {
    let f = fila(v)?;
    let ahora = now.unix_timestamp();
    let fallos = f.fallos_seguidos + 1;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    for id in &lote.ids {
        tx.execute(
            "UPDATE verifactu_record SET intentos = intentos + 1 WHERE id = ?1",
            [id],
        )?;
    }
    let (incidencia, proximo, resultado) = if del_mensaje {
        (f.incidencia_desde, None, "rechazado")
    } else {
        (
            Some(f.incidencia_desde.unwrap_or(ahora)),
            Some(ahora + espera_reintento(fallos)),
            "sin_respuesta",
        )
    };
    let error: String = error.chars().take(500).collect();
    tx.execute(
        "INSERT INTO verifactu_cola(id, proximo_envio, tiempo_espera, incidencia_desde,
           fallos_seguidos, ultimo_intento, ultimo_resultado, ultimo_error, bloqueada)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET proximo_envio = excluded.proximo_envio,
           incidencia_desde = excluded.incidencia_desde, fallos_seguidos = excluded.fallos_seguidos,
           ultimo_intento = excluded.ultimo_intento, ultimo_resultado = excluded.ultimo_resultado,
           ultimo_error = excluded.ultimo_error, bloqueada = excluded.bloqueada",
        params![
            proximo,
            f.tiempo_espera.max(ESPERA_INICIAL),
            incidencia,
            fallos,
            ahora,
            resultado,
            error,
            del_mensaje
        ],
    )?;
    audit::append(
        &tx,
        &invoicing::now_str(now),
        Actor::User,
        "verifactu.submit_failed",
        resultado,
        &serde_json::json!({ "records": lote.ids.len(), "error": error, "message": del_mensaje }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Desbloquea la cola tras un rechazo del mensaje (el usuario lo ha corregido) y deja enviar
/// ya. No envía nada.
pub fn reabrir(v: &Vault, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE verifactu_cola SET bloqueada = 0, proximo_envio = NULL WHERE id = 1",
        [],
    )?;
    audit::append(
        &tx,
        &invoicing::now_str(now),
        Actor::User,
        "verifactu.queue_reopen",
        "",
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Desde cuándo un SIF tiene que cumplir el RD 1007/2023 (DF 4.ª en la redacción del RDL
/// 15/2025): el 1/1/2027 los contribuyentes del Impuesto sobre Sociedades y el 1/7/2027 el resto.
/// `TODO(VERIFY)`: quien lleva el SII queda fuera (art. 3.2 del reglamento): TaxPain no lo sabe.
#[must_use]
pub fn obligado_desde(s: &InvoicingSettings) -> time::Date {
    use taxpain_core::invoice::PartyId;
    let persona_fisica = match &s.issuer.id {
        PartyId::Nif(n) => taxpain_core::nif::Nif::parse(n).map_or(true, |n| n.is_persona_fisica()),
        _ => true,
    };
    if persona_fisica {
        time::macros::date!(2027 - 07 - 01)
    } else {
        time::macros::date!(2027 - 01 - 01)
    }
}

/// Aviso cuando VERI*FACTU está desactivado. TaxPain es un sistema «sólo VERI*FACTU»
/// (`TipoUsoPosibleSoloVerifactu = S`): no tiene la modalidad sin remisión, que exige firmar
/// cada registro, llevar el registro de eventos y conservarlos y exportarlos (RD 1007/2023 arts.
/// 8 a 12; Orden HAC/1177/2024 arts. 7 a 14). Sin VERI*FACTU no genera registros, así que desde
/// la fecha de obligación no sirve para facturar.
#[must_use]
pub fn aviso_sin_verifactu(s: &InvoicingSettings, hoy: time::Date) -> Option<String> {
    if s.verifactu.enabled {
        return None;
    }
    let desde = obligado_desde(s);
    let fecha = format!(
        "{:02}/{:02}/{}",
        desde.day(),
        u8::from(desde.month()),
        desde.year()
    );
    Some(if hoy >= desde {
        format!(
            "VERI*FACTU está desactivado: estas facturas salen sin registro de facturación. \
             Desde el {fecha} un programa de facturación tiene que generarlo (RD 1007/2023) y \
             TaxPain sólo lo hace en modo VERI*FACTU (no tiene la modalidad sin remisión con \
             firma y registro de eventos). Actívalo en Ajustes › Facturación, salvo que lleves \
             el SII."
        )
    } else {
        format!(
            "VERI*FACTU está desactivado. Desde el {fecha} tendrás que facturar con un sistema \
             que genere los registros de facturación (RD 1007/2023): TaxPain sólo lo hace en \
             modo VERI*FACTU. Actívalo en Ajustes › Facturación antes de esa fecha, salvo que \
             lleves el SII."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_espera_entre_reintentos_crece_hasta_una_hora() {
        assert_eq!(espera_reintento(1), 60);
        assert_eq!(espera_reintento(2), 120);
        assert_eq!(espera_reintento(3), 240);
        assert_eq!(espera_reintento(7), 3600);
        assert_eq!(espera_reintento(1000), 3600);
    }
}
