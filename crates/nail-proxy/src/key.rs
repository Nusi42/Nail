//! The per-install signing key of proxy tokens.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nail_api::SecretRef;
use nail_api::engine::SecretStore;
use rand::RngCore;
use zeroize::Zeroize;

/// Length of a [`ProxyKey`] in bytes (HMAC-SHA-256 block-size friendly).
pub const KEY_LEN: usize = 32;

/// Keychain entry of the per-install key: `keychain:nail/proxy/signing-key`.
pub const KEY_SECRET_REF: &str = "keychain:nail/proxy/signing-key";

/// Errors loading or storing a [`ProxyKey`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    /// The keychain entry exists but is not a key of ours.
    #[error("the stored proxy key is malformed")]
    Malformed,
    /// The secret store failed.
    #[error("secret store: {0}")]
    Store(String),
}

/// A random 256-bit HMAC key. Only [`crate::Signer`] uses it; it never
/// leaves the process except into the OS keychain
/// ([`ProxyKey::load_or_create`]). `Debug` does not print it and it is
/// wiped on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyKey([u8; KEY_LEN]);

impl ProxyKey {
    /// A fresh key from the OS random number generator.
    pub fn generate() -> Self {
        let mut bytes = [0_u8; KEY_LEN];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// A key from raw bytes (tests, or a key restored by the caller).
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// The raw key.
    pub(crate) fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// The key as stored in the keychain (standard base64).
    fn encode(&self) -> String {
        STANDARD.encode(self.0)
    }

    fn decode(text: &str) -> Result<Self, KeyError> {
        let mut bytes = STANDARD
            .decode(text.trim())
            .map_err(|_| KeyError::Malformed)?;
        let key = <[u8; KEY_LEN]>::try_from(bytes.as_slice()).map_err(|_| KeyError::Malformed);
        bytes.zeroize();
        key.map(Self)
    }

    /// The per-install key: read from `secrets` at [`KEY_SECRET_REF`], or
    /// generated and stored there on first use. A malformed entry is an
    /// error (not silently replaced), so a damaged keychain is noticed.
    ///
    /// Blocking (keychain calls); call from `spawn_blocking` in async code.
    pub fn load_or_create(secrets: &dyn SecretStore) -> Result<Self, KeyError> {
        let secret = SecretRef(KEY_SECRET_REF.to_owned());
        match secrets
            .get(&secret)
            .map_err(|e| KeyError::Store(e.message))?
        {
            Some(text) => Self::decode(&text),
            None => {
                let key = Self::generate();
                secrets
                    .set(&secret, &key.encode())
                    .map_err(|e| KeyError::Store(e.message))?;
                Ok(key)
            }
        }
    }
}

impl Drop for ProxyKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for ProxyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProxyKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::MemorySecretStore;

    #[test]
    fn keys_are_random_and_redacted() {
        let a = ProxyKey::generate();
        let b = ProxyKey::generate();
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "ProxyKey(..)");
    }

    #[test]
    fn the_install_key_is_created_once_and_reused() {
        let secrets = MemorySecretStore::new();
        let first = ProxyKey::load_or_create(&secrets).unwrap();
        assert_eq!(secrets.len(), 1);
        let again = ProxyKey::load_or_create(&secrets).unwrap();
        assert_eq!(first, again);
    }

    #[test]
    fn a_damaged_entry_is_an_error() {
        let secrets = MemorySecretStore::new();
        let secret = SecretRef(KEY_SECRET_REF.to_owned());
        secrets.set(&secret, "not base64 !").unwrap();
        assert!(matches!(
            ProxyKey::load_or_create(&secrets),
            Err(KeyError::Malformed)
        ));
        secrets.set(&secret, &STANDARD.encode([1_u8; 16])).unwrap();
        assert!(matches!(
            ProxyKey::load_or_create(&secrets),
            Err(KeyError::Malformed)
        ));
        // Not replaced behind the user's back.
        assert_eq!(
            secrets.get(&secret).unwrap().unwrap(),
            STANDARD.encode([1_u8; 16])
        );
    }
}
