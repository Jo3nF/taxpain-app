/// Errores de la conexión con el banco. Ninguno lleva la URL ni datos de la cuenta.
#[derive(Debug, thiserror::Error)]
pub enum BancaError {
    /// La clave privada de la aplicación no sirve (no es PEM, no es RSA…).
    #[error("la clave privada de la aplicación no sirve: {0}")]
    Clave(String),
    /// No se ha podido abrir la conexión (sin internet, DNS…): la petición no ha salido.
    #[error("no se ha podido conectar con Enable Banking: {0}")]
    SinConexion(String),
    /// Ha fallado después de conectar.
    #[error("error de red con Enable Banking: {0}")]
    Red(String),
    /// El permiso del banco ha caducado o se ha retirado: hay que volver a autorizar.
    #[error("el permiso del banco ha caducado o se ha retirado: vuelve a conectar la cuenta")]
    Caducado,
    /// Enable Banking ha respondido con un error.
    #[error("Enable Banking ha respondido {estado}: {mensaje}")]
    Api {
        estado: u16,
        /// Código de error de Enable Banking («ACCESS_DENIED», «WRONG_REQUEST_PARAMETERS»…).
        codigo: Option<String>,
        mensaje: String,
    },
    /// La respuesta no tiene la forma esperada.
    #[error("respuesta de Enable Banking no válida: {0}")]
    Respuesta(String),
    /// La respuesta pasa del tope de lo que se lee: no se termina de leer.
    #[error("respuesta de Enable Banking demasiado grande: no se ha leído entera")]
    Grande,
    /// La dirección de vuelta del banco no trae el código, o trae un error.
    #[error("{0}")]
    Vuelta(String),
    #[error("TLS: {0}")]
    Tls(String),
}

pub type Result<T, E = BancaError> = std::result::Result<T, E>;
