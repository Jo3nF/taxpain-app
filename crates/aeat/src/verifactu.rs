//! Envío de registros VERI*FACTU al servicio web SOAP de la AEAT con el certificado del
//! usuario (mTLS). Endpoints del WSDL oficial (`fixtures/aeat/verifactu/SistemaFacturacion.wsdl`).
//!
//! Salvaguarda (D6): en builds de desarrollo sólo se puede enviar al entorno de **pruebas**;
//! producción exige build de release **y** autorización explícita (`Destino::produccion`),
//! que la app sólo concede tras la confirmación del usuario.

use taxpain_invoicing::verifactu::{self, Response};

use crate::http::{AeatClient, AeatError};

pub const URL_PRUEBAS: &str =
    "https://prewww1.aeat.es/wlpl/TIKE-CONT/ws/SistemaFacturacion/VerifactuSOAP";
pub const URL_PRODUCCION: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/TIKE-CONT/ws/SistemaFacturacion/VerifactuSOAP";

/// A dónde se envía. Sólo se construye con los métodos (no hay producción por accidente).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destino {
    url: String,
}

impl Destino {
    pub fn pruebas() -> Self {
        Destino {
            url: URL_PRUEBAS.into(),
        }
    }

    /// Producción: rechazado en builds de desarrollo.
    pub fn produccion(confirmado_por_usuario: bool) -> Result<Self, AeatError> {
        if cfg!(debug_assertions) {
            return Err(AeatError::ProductionBlocked("build de desarrollo"));
        }
        if !confirmado_por_usuario {
            return Err(AeatError::ProductionBlocked(
                "falta la confirmación del usuario",
            ));
        }
        Ok(Destino {
            url: URL_PRODUCCION.into(),
        })
    }

    /// Servidor simulado (tests). Sólo con la feature `mock`: la app no lo lleva.
    #[cfg(any(test, feature = "mock"))]
    pub fn mock(url: &str) -> Self {
        Destino { url: url.into() }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Envía `body` (`sfLR:RegFactuSistemaFacturacion`) y devuelve la respuesta interpretada.
/// La AEAT responde los errores de esquema con un SOAP Fault (HTTP 500).
pub async fn enviar(
    client: &AeatClient,
    destino: &Destino,
    body: &str,
) -> Result<Response, AeatError> {
    let envelope = verifactu::soap_envelope(body);
    let (status, text) = client.post_soap(destino.url(), &envelope).await?;
    verifactu::parse_response(&text).map_err(|e| {
        if (200..300).contains(&status) || status == 500 {
            AeatError::InvalidResponse(e.to_string())
        } else {
            AeatError::Status(status)
        }
    })
}

/// Consulta los registros que tiene la AEAT (`sfLRC:ConsultaFactuSistemaFacturacion`, mismo
/// servicio que el envío). Sólo lectura.
pub async fn consultar(
    client: &AeatClient,
    destino: &Destino,
    body: &str,
) -> Result<verifactu::ConsultaRespuesta, AeatError> {
    let envelope = verifactu::soap_envelope(body);
    let (status, text) = client.post_soap(destino.url(), &envelope).await?;
    verifactu::parse_consulta(&text).map_err(|e| {
        if (200..300).contains(&status) || status == 500 {
            AeatError::InvalidResponse(e.to_string())
        } else {
            AeatError::Status(status)
        }
    })
}

/// Si la AEAT rechazó el **mensaje** (`SOAP Fault` `Client`: esquema o cabecera): hay que
/// corregirlo antes de reenviar, así que la cola no lo reintenta sola. Cualquier otro error
/// (sin red, sin respuesta, `Server`, HTTP) es una incidencia técnica y se reintenta
/// (Orden HAC/1177/2024 art. 16.4; [SW] 5.1).
#[must_use]
pub fn fallo_del_mensaje(e: &AeatError) -> bool {
    matches!(e, AeatError::InvalidResponse(m) if verifactu::fault_is_client(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distingue_el_rechazo_del_mensaje_de_la_incidencia_tecnica() {
        let client = AeatError::InvalidResponse(
            "respuesta de la AEAT no válida: SOAP Fault (env:Client): Codigo[4102]".into(),
        );
        let server = AeatError::InvalidResponse(
            "respuesta de la AEAT no válida: SOAP Fault (soapenv:Server): caído".into(),
        );
        assert!(fallo_del_mensaje(&client));
        assert!(!fallo_del_mensaje(&server));
        assert!(!fallo_del_mensaje(&AeatError::Status(503)));
    }

    #[test]
    fn produccion_bloqueada_en_desarrollo() {
        if cfg!(debug_assertions) {
            assert!(Destino::produccion(true).is_err());
        }
        assert!(Destino::produccion(false).is_err());
        assert_eq!(Destino::pruebas().url(), URL_PRUEBAS);
    }
}
