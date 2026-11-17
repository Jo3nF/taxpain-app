#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AssistantError {
    #[error("dirección del proveedor no válida")]
    InvalidUrl,
    #[error(
        "el asistente sólo puede conectarse a proveedores conocidos o a esta máquina (no a {0})"
    )]
    HostNotAllowed(String),
    #[error("falta la clave de API del proveedor")]
    MissingKey,
    #[error("no se envía: no se han podido proteger los datos personales ({0})")]
    Protection(String),
    #[error("no se envía: el mensaje {0}")]
    SecretDetected(&'static str),
    /// Un secreto en una parte con nombre del envío («el dato 2», «el documento 3», «el tema»).
    #[error("no se envía: {0} {1}")]
    SecretIn(String, &'static str),
    #[error("error de red: {0}")]
    Network(String),
    /// El proveedor no ha respondido en el tiempo de espera del cliente (`provider::TIMEOUT`).
    #[error("El proveedor no ha respondido en 3 minutos: vuelve a preguntar.")]
    Timeout,
    #[error("el proveedor respondió: {0}")]
    Provider(String),
    /// La respuesta pasa del tope de lo que se lee: no se termina de leer.
    #[error("respuesta demasiado grande: no se ha leído entera")]
    TooLarge,
    #[error("el asistente pidió una herramienta que no existe ({0}): se ha parado la consulta")]
    UnknownTool(String),
}
