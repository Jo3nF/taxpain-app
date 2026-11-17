use taxpain_core::invoice::ValidationError;
use taxpain_invoicing::facturae::FacturaeError;
use taxpain_invoicing::verifactu::VerifactuError;
use taxpain_vault::VaultError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("factura no válida: {0}")]
    Invalid(#[from] ValidationError),
    #[error("Facturae: {0}")]
    Facturae(#[from] FacturaeError),
    #[error("VERI*FACTU: {0}")]
    Verifactu(#[from] VerifactuError),
    #[error("configura primero los datos de facturación")]
    NotConfigured,
    #[error("no existe la serie {0}")]
    UnknownSeries(String),
    #[error("no encontrado")]
    NotFound,
    #[error("hace falta confirmar la operación")]
    NotConfirmed,
    #[error("fichero del modelo: {0}")]
    Model(#[from] taxpain_models::dr::DrError),
    #[error(transparent)]
    Banca(#[from] taxpain_banca::BancaError),
    #[error(transparent)]
    Assistant(#[from] taxpain_assistant::AssistantError),
    #[error("{0}")]
    Other(String),
}

impl From<rusqlite_error::Error> for AppError {
    fn from(e: rusqlite_error::Error) -> Self {
        AppError::Vault(VaultError::Db(e))
    }
}

/// Reexportación local para no depender directamente de rusqlite en la API pública.
pub(crate) mod rusqlite_error {
    pub use taxpain_vault::rusqlite::Error;
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;
