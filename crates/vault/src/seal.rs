//! Sellado AES-256-GCM de blobs secretos con datos asociados (AAD) que los ligan a su dueño.
//!
//! Formato: `"TPS1"` ‖ nonce (12 bytes aleatorios) ‖ texto cifrado ‖ etiqueta (16 bytes).
//! La AAD incluye la cabecera y el contexto (p. ej. la huella del certificado), así que un
//! blob no se puede mover a otro registro sin que falle el descifrado.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::error::{Result, VaultError};
use taxpain_secrets::{Key256, SecretBytes};

const MAGIC: &[u8; 4] = b"TPS1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

fn aad(context: &[u8]) -> Vec<u8> {
    [MAGIC.as_slice(), context].concat()
}

pub fn seal(key: &Key256, context: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(&(*key.expose()).into());
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|e| VaultError::KeyUnavailable(e.to_string()))?;
    let aad = aad(context);
    let ct = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| VaultError::Corrupted)?;
    Ok([MAGIC.as_slice(), &nonce, &ct].concat())
}

pub fn unseal(key: &Key256, context: &[u8], sealed: &[u8]) -> Result<SecretBytes> {
    if sealed.len() < MAGIC.len() + NONCE_LEN + TAG_LEN || &sealed[..4] != MAGIC {
        return Err(VaultError::Corrupted);
    }
    let nonce: [u8; NONCE_LEN] = sealed[4..4 + NONCE_LEN]
        .try_into()
        .map_err(|_| VaultError::Corrupted)?;
    let cipher = Aes256Gcm::new(&(*key.expose()).into());
    let aad = aad(context);
    cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &sealed[4 + NONCE_LEN..],
                aad: &aad,
            },
        )
        .map(SecretBytes::new)
        .map_err(|_| VaultError::Corrupted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_binding() {
        let k = Key256::random().unwrap();
        let sealed = seal(&k, b"cert:aa", b"secreto").unwrap();
        assert!(!sealed.windows(7).any(|w| w == b"secreto"));
        assert_eq!(
            unseal(&k, b"cert:aa", &sealed).unwrap().expose(),
            b"secreto"
        );
        // Otro contexto, otra clave o un bit cambiado → Corrupted.
        assert!(matches!(
            unseal(&k, b"cert:bb", &sealed),
            Err(VaultError::Corrupted)
        ));
        let other = Key256::random().unwrap();
        assert!(matches!(
            unseal(&other, b"cert:aa", &sealed),
            Err(VaultError::Corrupted)
        ));
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(matches!(
            unseal(&k, b"cert:aa", &tampered),
            Err(VaultError::Corrupted)
        ));
        assert!(matches!(
            unseal(&k, b"cert:aa", b"TPS1"),
            Err(VaultError::Corrupted)
        ));
    }

    #[test]
    fn nonces_are_random() {
        let k = Key256::random().unwrap();
        assert_ne!(seal(&k, b"", b"x").unwrap(), seal(&k, b"", b"x").unwrap());
    }
}
