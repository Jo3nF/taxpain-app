//! Integración con macOS: `KeychainKeyProvider` (claves en el Keychain), `TouchIdPresence`
//! (LocalAuthentication) y `move_to_trash` (Papelera). En otras plataformas el crate está vacío.
//! Ver `docs/DECISIONS.md` D4 y D9.
//!
//! Este código sólo se compila (`cargo check --target aarch64-apple-darwin`) desde el entorno
//! Linux de desarrollo. `TODO(VERIFY)`: probarlo en un Mac real (Keychain, Touch ID, Mac sin
//! Touch ID con contraseña de usuario) antes del MVP.

#[cfg(target_os = "macos")]
mod keychain;
#[cfg(target_os = "macos")]
mod touch_id;
#[cfg(target_os = "macos")]
mod trash;

#[cfg(target_os = "macos")]
pub use keychain::{KeychainKeyProvider, KeychainMode};
#[cfg(target_os = "macos")]
pub use touch_id::TouchIdPresence;
#[cfg(target_os = "macos")]
pub use trash::move_to_trash;
#[cfg(target_os = "macos")]
pub mod ocr;
