//! Encryption at rest for Nail (M2a WP33; design and evaluation in
//! `docs/adr/0032-m2a-data-protection.md`).
//!
//! * **Format** ([`format`](mod@format)): one sealed-data format for everything —
//!   chunked XChaCha20-Poly1305 with a random per-item nonce prefix, bound
//!   to where the data belongs (associated data), truncation-safe, random
//!   access by chunk.
//! * **Key** ([`key`]): a random 256-bit data key in the OS keychain
//!   (through [`nail_api::engine::SecretStore`], never on disk), under a
//!   random `keychain:nail/vault-…/data-key` reference, with a key check
//!   value in the store's `vault_state` row. [`VaultCipher`] implements
//!   [`nail_store::DataCipher`].
//! * **Database**: the sensitive columns ([`nail_store::SEALED_COLUMNS`])
//!   are sealed by the store's hooks (M2a). Since M4c (WP122, ADR
//!   0095) the whole database file is encrypted as well: the bundled
//!   SQLite is SQLite3 Multiple Ciphers (a `[patch]` of `libsqlite3-sys`),
//!   keyed with [`key::database_key`]; [`open_data_dir`] opens it and the
//!   vault file ([`vaultfile`]) keeps what opening and recovery need
//!   readable beside it.
//! * **Recovery key** ([`recovery`], M4c): a 256-bit key shown once that
//!   wraps the data key, to restore it after the keychain entry was lost.
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

mod datadir;
pub mod directory;
mod error;
pub mod format;
pub mod key;
pub mod recovery;
mod vault;
pub mod vaultfile;

use nail_api::security::VaultState;

pub use datadir::{LockedVault, OpenedDataDir, ReadyDataDir, open_data_dir};
pub use directory::SealedDirectory;
pub use error::VaultError;
pub use format::DataKey;
pub use key::{KeyStore, VaultCipher};
pub use recovery::RecoveryKey;
pub use vault::{INDEX_VERSION_FILE, IndexWrapper, Vault};
pub use vaultfile::{VAULT_FILE_NAME, VaultFile};

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
