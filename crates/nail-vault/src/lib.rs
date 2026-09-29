//! Encryption at rest for Nail (M2a WP33; design and evaluation in
//! `docs/adr/0032-m2a-data-protection.md`).
//!
//! * **Format** ([`format`]): one sealed-data format for everything —
//!   chunked XChaCha20-Poly1305 with a random per-item nonce prefix, bound
//!   to where the data belongs (associated data), truncation-safe, random
//!   access by chunk.
//! * **Key** ([`key`]): a random 256-bit data key in the OS keychain
//!   (through [`nail_api::engine::SecretStore`], never on disk), under a
//!   random `keychain:nail/vault-…/data-key` reference, with a key check
//!   value in the store's `vault_state` row. [`VaultCipher`] implements
//!   [`nail_store::DataCipher`].
//! * **Database**: SQLCipher needs OpenSSL on Windows MSVC and
//!   SQLite3MultipleCiphers is not buildable through `rusqlite` without
//!   patching `libsqlite3-sys`, so the database stays a plain SQLite file
//!   and the sensitive columns ([`nail_store::SEALED_COLUMNS`]) are sealed
//!   by the store's hooks.
//! * **Blobs**: sealed after compression by [`nail_store::BlobStore`] once
//!   the cipher is installed.
//! * **Search index**: [`SealedDirectory`], an encrypting tantivy
//!   `Directory` ([`Vault::index_wrapper`]).
//! * **Migration** ([`Vault`]): plain ↔ encrypted, database → blobs →
//!   index, resumable after a crash from the `vault_state` row, progress
//!   as [`nail_api::security::VaultStatus`].
//!
//! Encryption at rest ships **off** by default until the maintainer decides
//! (plan section 7); the UI recommends turning it on.

#![warn(missing_docs)]

pub mod directory;
mod error;
pub mod format;
pub mod key;
mod vault;

use nail_api::security::VaultState;

pub use directory::SealedDirectory;
pub use error::VaultError;
pub use format::DataKey;
pub use key::{KeyStore, VaultCipher};
pub use vault::{INDEX_VERSION_FILE, IndexWrapper, Vault};

/// The state of a data directory nobody has encrypted: encryption at rest
/// is **off** by default.
pub const DEFAULT_STATE: VaultState = VaultState::Plain;

#[cfg(test)]
mod tests {
    #[test]
    fn default_is_plain() {
        assert_eq!(super::DEFAULT_STATE, nail_api::security::VaultState::Plain);
    }
}
