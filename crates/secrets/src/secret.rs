//! Material secreto en memoria: se borra al soltarse (`zeroize`) y nunca se imprime.

use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{Error, Result};

/// Clave simétrica de 256 bits.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Key256([u8; 32]);

impl Key256 {
    pub fn random() -> Result<Self> {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).map_err(|e| Error::KeyUnavailable(e.to_string()))?;
        Ok(Key256(k))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::KeyUnavailable("longitud de clave incorrecta".into()))?;
        Ok(Key256(arr))
    }

    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    /// Representación hexadecimal para `PRAGMA key` (se borra al soltarse).
    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(self.0))
    }
}

impl fmt::Debug for Key256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key256([REDACTED])")
    }
}

/// Bytes secretos de longitud variable (clave privada PKCS#8, contraseñas).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        SecretBytes(bytes)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes([REDACTED; {}])", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_secrets() {
        let k = Key256::from_bytes(&[0xAB; 32]).unwrap();
        assert!(!format!("{k:?}").to_lowercase().contains("ab"));
        let s = SecretBytes::new(b"hunter2".to_vec());
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    #[test]
    fn random_keys_differ() {
        assert_ne!(
            Key256::random().unwrap().expose(),
            Key256::random().unwrap().expose()
        );
    }
}
