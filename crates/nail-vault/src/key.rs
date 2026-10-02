//! The data key: created once, kept in the OS keychain, checked against a
//! key check value in the store, and used through [`VaultCipher`].

use std::sync::Arc;

use nail_api::SecretRef;
use nail_api::engine::SecretStore;
use nail_store::{CipherError, DataCipher};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::error::VaultError;
use crate::format::{self, DataKey};

/// Context of the key check value.
const KEY_CHECK_CONTEXT: &[u8] = b"nail/vault/key-check";
/// Plaintext of the key check value.
const KEY_CHECK_PLAIN: &[u8] = b"nail vault key check v1";

/// A fresh keychain reference for a data key:
/// `keychain:nail/vault-<16 hex>/data-key`. Random per data directory, so
/// several profiles (or test runs) never share or overwrite a key.
pub fn new_key_ref() -> SecretRef {
    let mut id = [0u8; 8];
    OsRng.fill_bytes(&mut id);
    SecretRef(format!(
        "{}{}/vault-{}/data-key",
        SecretRef::SCHEME,
        SecretRef::SERVICE,
        hex::encode(id)
    ))
}

/// The data key in the OS keychain, through the engine trait
/// [`SecretStore`] (`nail-auth` implements it with the Windows Credential
/// Manager; tests use `MemorySecretStore`).
#[derive(Debug, Clone)]
pub struct KeyStore {
    secrets: Arc<dyn SecretStore>,
}

impl KeyStore {
    /// Wraps a secret store.
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }

    /// Stores a key under `key_ref` (replacing an entry).
    pub fn store(&self, key_ref: &SecretRef, key: &DataKey) -> Result<(), VaultError> {
        self.secrets.set(key_ref, &key.to_hex())?;
        Ok(())
    }

    /// Loads the key; [`VaultError::Locked`] when there is no entry.
    pub fn load(&self, key_ref: &SecretRef) -> Result<DataKey, VaultError> {
        let text = self.secrets.get(key_ref)?.ok_or_else(|| {
            VaultError::Locked(format!(
                "the data key {} is not in the keychain",
                key_ref.as_str()
            ))
        })?;
        let text = Zeroizing::new(text);
        DataKey::from_hex(&text)
    }

    /// Whether an entry exists (a failing keychain counts as "no").
    pub fn contains(&self, key_ref: &SecretRef) -> bool {
        matches!(self.secrets.get(key_ref), Ok(Some(_)))
    }

    /// Deletes the entry. Returns whether it existed.
    pub fn delete(&self, key_ref: &SecretRef) -> Result<bool, VaultError> {
        Ok(self.secrets.delete(key_ref)?)
    }
}

/// The key check value stored next to the key reference: a sealed known
/// plaintext. It reveals nothing about the key but tells a wrong key from
/// damaged data.
pub fn key_check(key: &DataKey) -> Result<Vec<u8>, VaultError> {
    format::seal(key, KEY_CHECK_CONTEXT, KEY_CHECK_PLAIN)
}

/// Verifies [`key_check`].
pub fn verify_key_check(key: &DataKey, check: &[u8]) -> Result<(), VaultError> {
    match format::open(key, KEY_CHECK_CONTEXT, check) {
        Ok(plain) if plain == KEY_CHECK_PLAIN => Ok(()),
        _ => Err(VaultError::Crypto(
            "the data key in the keychain does not match this data directory".into(),
        )),
    }
}

/// Context of the database-file key derivation.
const DATABASE_KEY_CONTEXT: &[u8] = b"nail/vault/database-file/v1";

/// A 256-bit key derived from `key` for a purpose named by `context`
/// (SHA-256 over the context, a separator and the key; the inputs are
/// uniformly random 256-bit keys, so no slow KDF is needed).
pub(crate) fn derive(key: &[u8; 32], context: &[u8]) -> Zeroizing<[u8; 32]> {
    use sha2::Digest;
    let digest = sha2::Sha256::new()
        .chain_update(context)
        .chain_update([0u8])
        .chain_update(key)
        .finalize();
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&digest);
    out
}

/// The key of the full-file encrypted database (SQLite3 Multiple
/// Ciphers), derived from the data key so the keychain holds one secret.
pub fn database_key(key: &DataKey) -> nail_store::DatabaseKey {
    nail_store::DatabaseKey::from_bytes(*derive(key.bytes(), DATABASE_KEY_CONTEXT))
}

/// The [`DataCipher`] the store and the blob store use.
#[derive(Debug, Clone)]
pub struct VaultCipher {
    key: DataKey,
}

impl VaultCipher {
    /// A cipher over a data key.
    pub fn new(key: DataKey) -> Self {
        Self { key }
    }

    /// The key.
    pub fn key(&self) -> &DataKey {
        &self.key
    }
}

impl DataCipher for VaultCipher {
    fn seal(&self, context: &[u8], plain: &[u8]) -> Result<Vec<u8>, CipherError> {
        Ok(format::seal(&self.key, context, plain)?)
    }

    fn open(&self, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CipherError> {
        Ok(format::open(&self.key, context, sealed)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::MemorySecretStore;

    #[test]
    fn key_refs_are_valid_and_unique() {
        let a = new_key_ref();
        let b = new_key_ref();
        assert_ne!(a, b);
        assert!(SecretRef::parse(a.as_str()).is_ok(), "{a:?}");
        assert!(a.as_str().starts_with("keychain:nail/vault-"));
    }

    #[test]
    fn keychain_round_trip_and_missing_key_is_locked() {
        let store = KeyStore::new(Arc::new(MemorySecretStore::new()));
        let key_ref = new_key_ref();
        assert!(matches!(store.load(&key_ref), Err(VaultError::Locked(_))));
        assert!(!store.contains(&key_ref));
        let key = DataKey::generate();
        store.store(&key_ref, &key).unwrap();
        assert!(store.contains(&key_ref));
        let loaded = store.load(&key_ref).unwrap();
        let check = key_check(&key).unwrap();
        verify_key_check(&loaded, &check).unwrap();
        assert!(matches!(
            verify_key_check(&DataKey::generate(), &check),
            Err(VaultError::Crypto(_))
        ));
        assert!(store.delete(&key_ref).unwrap());
        assert!(!store.delete(&key_ref).unwrap());
    }

    #[test]
    fn database_keys_are_derived_deterministically_and_separately() {
        let key = DataKey::from_bytes([1; 32]);
        assert_eq!(database_key(&key), database_key(&key));
        assert_ne!(
            database_key(&key),
            database_key(&DataKey::from_bytes([2; 32]))
        );
        // Not the data key itself, and different per context.
        assert_ne!(*derive(key.bytes(), DATABASE_KEY_CONTEXT), [1u8; 32]);
        assert_ne!(
            *derive(key.bytes(), DATABASE_KEY_CONTEXT),
            *derive(key.bytes(), b"nail/other")
        );
    }

    #[test]
    fn cipher_output_carries_the_store_magic() {
        let cipher = VaultCipher::new(DataKey::generate());
        let sealed = cipher.seal(b"nail/blob/x", b"content").unwrap();
        assert!(sealed.starts_with(&nail_store::SEALED_MAGIC));
        assert_eq!(cipher.open(b"nail/blob/x", &sealed).unwrap(), b"content");
        assert!(matches!(
            cipher.open(b"nail/blob/y", &sealed),
            Err(CipherError::Failed(_))
        ));
    }
}
