//! Confirmación con Touch ID (o contraseña del usuario en Macs sin Touch ID) mediante
//! `LAContext.evaluatePolicy(.deviceOwnerAuthentication, localizedReason:)`.

#![allow(unsafe_code)] // llamadas Objective-C a LocalAuthentication

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use taxpain_secrets::{Error, Result, UserPresence};

/// Tiempo máximo esperando al usuario.
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Default)]
pub struct TouchIdPresence;

impl TouchIdPresence {
    /// `true` si el Mac puede autenticar al propietario (Touch ID o contraseña).
    pub fn is_available() -> bool {
        // SAFETY: LAContext se crea y usa en este hilo; canEvaluatePolicy no tiene efectos.
        unsafe {
            LAContext::new()
                .canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication)
                .is_ok()
        }
    }
}

impl UserPresence for TouchIdPresence {
    /// No llamar desde el hilo principal: bloquea hasta que el usuario responde.
    fn confirm(&self, reason: &str) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel::<bool>(1);
        let reply = RcBlock::new(move |ok: Bool, _err: *mut NSError| {
            let _ = tx.try_send(ok.as_bool());
        });
        // SAFETY: el bloque es `Send`-safe (sólo envía por un canal) y vive hasta que se llama;
        // `context` sigue vivo mientras esperamos la respuesta.
        let context = unsafe { LAContext::new() };
        unsafe {
            context.evaluatePolicy_localizedReason_reply(
                LAPolicy::DeviceOwnerAuthentication,
                &NSString::from_str(reason),
                &reply,
            );
        }
        match rx.recv_timeout(TIMEOUT) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Error::NotConfirmed),
            Err(_) => {
                // SAFETY: invalidar cancela el diálogo pendiente.
                unsafe { context.invalidate() };
                Err(Error::NotConfirmed)
            }
        }
    }
}
