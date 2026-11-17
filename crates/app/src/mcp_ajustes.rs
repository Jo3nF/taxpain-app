//! Acceso de Claude a los datos de la empresa por el servidor MCP (`taxpain-mcp`): el ajuste que
//! lo permite y lo que deja anotado.
//!
//! El ajuste va **dentro de la BD cifrada de cada empresa** (no en un fichero suelto que
//! cualquiera pudiera cambiar) y está apagado por defecto: sin activarlo en la app, el servidor
//! no enseña nada de esa empresa, ni siquiera que existe. Tiene dos niveles: sólo consultar, o
//! consultar y dejar borradores (un presupuesto, una factura recibida pendiente de revisar).
//!
//! Lo que sale por el servidor va **sin anonimizar** (a diferencia del asistente de la app): lo
//! dice la sección de Ajustes donde se activa y `docs/PRIVACIDAD_APP.md`.
//!
//! En la auditoría: `mcp.ajustes` (el usuario lo activa, lo apaga o cambia de nivel) y
//! `mcp.call` (una por consulta del servidor: la herramienta y la empresa, nunca los argumentos
//! ni lo que devuelve). Lo que hace el servidor queda con el actor [`Actor::Assistant`].

use serde::{Deserialize, Serialize};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Transaction, TransactionBehavior};
use time::OffsetDateTime;

use crate::error::Result;
use crate::invoicing::now_str;

/// Ajuste (tabla `settings`) con [`Ajustes`].
pub const AJUSTES: &str = "mcp";

/// Nota que lleva lo que deja Claude por el servidor, para que se vea al revisarlo en la app.
pub const NOTA_PRESUPUESTO: &str = "Creado por Claude (MCP): revísalo antes de enviarlo.";
pub const NOTA_RECIBIDA: &str =
    "subida por Claude (MCP): comprueba que es una factura de la empresa";

/// Hasta dónde llega Claude en una empresa que lo tiene activado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nivel {
    /// Sólo consultar.
    #[default]
    Consultar,
    /// Consultar y dejar borradores que el usuario revisa en la app.
    Borradores,
}

impl Nivel {
    fn as_str(self) -> &'static str {
        match self {
            Nivel::Consultar => "consultar",
            Nivel::Borradores => "borradores",
        }
    }
}

/// El ajuste de una empresa. Por defecto, apagado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ajustes {
    #[serde(default)]
    pub activado: bool,
    #[serde(default)]
    pub nivel: Nivel,
}

/// El ajuste guardado. Si no hay, o lo guardado no se entiende (lo escribió otra versión), es
/// como si estuviera apagado: ante la duda, Claude no entra.
pub fn ajustes(v: &Vault) -> Result<Ajustes> {
    match v.setting::<Ajustes>(AJUSTES) {
        Ok(a) => Ok(a.unwrap_or_default()),
        Err(taxpain_vault::VaultError::InvalidInput(e)) => {
            tracing::warn!(error = %e, "el ajuste del servidor MCP no se entiende: apagado");
            Ok(Ajustes::default())
        }
        Err(e) => Err(e.into()),
    }
}

/// El nivel con el que entra Claude, o `None` si la empresa no lo tiene activado.
pub fn nivel(v: &Vault) -> Result<Option<Nivel>> {
    let a = ajustes(v)?;
    Ok(a.activado.then_some(a.nivel))
}

/// Guarda el ajuste (lo hace el usuario desde Ajustes → Claude (MCP)). Si cambia algo, queda en
/// la auditoría, en la misma transacción.
pub fn guardar(v: &Vault, a: &Ajustes, now: OffsetDateTime) -> Result<()> {
    let tx = Transaction::new_unchecked(v.connection(), TransactionBehavior::Immediate)?;
    // Sin cambios (o apagarlo sin haberlo activado nunca): nada que guardar ni que anotar.
    if ajustes(v)? == *a {
        return Ok(());
    }
    v.set_setting(AJUSTES, a)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "mcp.ajustes",
        "claude (mcp)",
        &serde_json::json!({ "activado": a.activado, "nivel": a.nivel.as_str() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Anota una llamada del servidor MCP: qué herramienta y en qué empresa. Ni los argumentos ni
/// lo que devuelve (pueden llevar datos de terceros). Coge el cerrojo de escritura antes de leer
/// la cabeza de la cadena, para no chocar con la app si escribe a la vez.
pub fn anotar_llamada(
    v: &Vault,
    herramienta: &str,
    empresa: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let tx = Transaction::new_unchecked(v.connection(), TransactionBehavior::Immediate)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::Assistant,
        "mcp.call",
        herramienta,
        &serde_json::json!({ "empresa": empresa }),
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use taxpain_vault::{MemoryKeyProvider, NeverConfirm};
    use time::macros::datetime;

    use super::*;

    const AHORA: OffsetDateTime = datetime!(2026-11-12 10:00 UTC);

    fn almacen() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(NeverConfirm),
        )
        .unwrap()
    }

    fn acciones(v: &Vault) -> Vec<(String, String, String, String)> {
        audit::entries(v.connection())
            .unwrap()
            .into_iter()
            .map(|e| (e.actor, e.action, e.subject, e.details))
            .collect()
    }

    #[test]
    fn apagado_por_defecto() {
        let v = almacen();
        assert_eq!(ajustes(&v).unwrap(), Ajustes::default());
        assert!(!ajustes(&v).unwrap().activado);
        assert_eq!(nivel(&v).unwrap(), None);
        assert!(acciones(&v).is_empty());
    }

    #[test]
    fn activar_y_cambiar_de_nivel_queda_en_la_auditoria() {
        let v = almacen();
        let consultar = Ajustes {
            activado: true,
            nivel: Nivel::Consultar,
        };
        guardar(&v, &consultar, AHORA).unwrap();
        assert_eq!(nivel(&v).unwrap(), Some(Nivel::Consultar));
        // Guardar lo mismo no añade otra entrada.
        guardar(&v, &consultar, AHORA).unwrap();
        let borradores = Ajustes {
            activado: true,
            nivel: Nivel::Borradores,
        };
        guardar(&v, &borradores, AHORA).unwrap();
        assert_eq!(nivel(&v).unwrap(), Some(Nivel::Borradores));
        guardar(
            &v,
            &Ajustes {
                activado: false,
                nivel: Nivel::Borradores,
            },
            AHORA,
        )
        .unwrap();
        assert_eq!(nivel(&v).unwrap(), None);
        let log = acciones(&v);
        assert_eq!(log.len(), 3);
        assert!(log.iter().all(|(actor, action, subject, _)| actor == "user"
            && action == "mcp.ajustes"
            && subject == "claude (mcp)"));
        assert_eq!(log[0].3, r#"{"activado":true,"nivel":"consultar"}"#);
        assert_eq!(log[1].3, r#"{"activado":true,"nivel":"borradores"}"#);
        assert_eq!(log[2].3, r#"{"activado":false,"nivel":"borradores"}"#);
        v.verify_audit().unwrap();
    }

    #[test]
    fn un_ajuste_que_no_se_entiende_es_apagado() {
        let v = almacen();
        v.set_setting(
            AJUSTES,
            &serde_json::json!({ "activado": "sí", "nivel": 7 }),
        )
        .unwrap();
        assert_eq!(ajustes(&v).unwrap(), Ajustes::default());
        assert_eq!(nivel(&v).unwrap(), None);
        // Un nivel que esta versión no conoce tampoco abre la puerta.
        v.set_setting(
            AJUSTES,
            &serde_json::json!({ "activado": true, "nivel": "todo" }),
        )
        .unwrap();
        assert_eq!(nivel(&v).unwrap(), None);
    }

    #[test]
    fn la_llamada_se_anota_sin_argumentos_y_como_asistente() {
        let v = almacen();
        anotar_llamada(&v, "facturas_emitidas", "principal", AHORA).unwrap();
        assert_eq!(
            acciones(&v),
            [(
                "assistant".to_owned(),
                "mcp.call".to_owned(),
                "facturas_emitidas".to_owned(),
                r#"{"empresa":"principal"}"#.to_owned()
            )]
        );
        v.verify_audit().unwrap();
    }
}
