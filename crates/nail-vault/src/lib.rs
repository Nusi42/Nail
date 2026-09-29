//! Encryption at rest for Nail (M2a WP33; stub after WP30, ADR 0029).
//!
//! Expected shape:
//!
//! * a data key (random, 256 bit) kept in the OS keychain
//!   (`nail_api::engine::SecretStore`), never on disk; a key check value in
//!   the store's `vault_state` row detects a wrong or missing key;
//! * the blob store's files encrypted with XChaCha20-Poly1305
//!   ([`chacha20poly1305`]) under a small versioned header, so plain and
//!   encrypted blobs can be told apart during a migration;
//! * the search index through an encrypting tantivy `Directory`;
//! * the SQLite database: evaluate SQLCipher-compatible options **without
//!   an OpenSSL build dependency** (ADR required if none exists) against
//!   application-level encryption of the sensitive columns;
//! * a crash-safe migration plain ↔ encrypted that resumes from
//!   `vault_state` (`Store::vault_state` / `Store::set_vault_state`,
//!   phases database → blobs → index) and reports progress as
//!   [`nail_api::security::VaultStatus`];
//! * the decrypted-content indexing policy
//!   ([`nail_api::security::SecuritySettings::index_decrypted`]).
//!
//! Encryption at rest ships **off** by default until the maintainer decides
//! (plan section 7); the UI recommends turning it on.

#![warn(missing_docs)]

use nail_api::security::VaultState;

/// The state of a data directory nobody has encrypted.
pub const DEFAULT_STATE: VaultState = VaultState::Plain;

#[cfg(test)]
mod tests {
    //! Smoke tests: the chosen libraries build on Windows MSVC and work
    //! (WP30 verifies them before WP33 relies on them).

    use chacha20poly1305::XChaCha20Poly1305;
    use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
    use rand::rngs::OsRng;
    use zeroize::Zeroize;

    #[test]
    fn default_is_plain() {
        assert_eq!(super::DEFAULT_STATE, nail_api::security::VaultState::Plain);
    }

    #[test]
    fn xchacha20poly1305_round_trips_and_detects_tampering() {
        let key = XChaCha20Poly1305::generate_key(&mut OsRng);
        let cipher = XChaCha20Poly1305::new(&key);
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut sealed = cipher.encrypt(&nonce, b"message body".as_ref()).unwrap();
        assert_eq!(
            cipher.decrypt(&nonce, sealed.as_ref()).unwrap(),
            b"message body"
        );
        sealed[0] ^= 1;
        assert!(cipher.decrypt(&nonce, sealed.as_ref()).is_err());
        let mut secret = key.to_vec();
        secret.zeroize();
        assert!(secret.iter().all(|b| *b == 0));
    }

    #[test]
    fn tantivy_directory_trait_is_available() {
        // The encrypted index wraps a `Directory`; the RAM one is enough to
        // show the trait and the crate build here.
        use tantivy::directory::{Directory, RamDirectory};
        let dir = RamDirectory::create();
        let path = std::path::Path::new("meta.json");
        dir.atomic_write(path, b"{}").unwrap();
        assert_eq!(dir.atomic_read(path).unwrap(), b"{}");
    }
}
