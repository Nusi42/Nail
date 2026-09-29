//! Local store: SQLite metadata, a content-addressed blob store and the
//! operation queue (design.md, "Storage, sync & search").
//!
//! * [`Store`] — the metadata database (SQLite, bundled, WAL mode) with
//!   numbered migrations ([`SCHEMA_VERSION`]). It holds accounts, folders
//!   (with IMAP sync state) and messages, and exposes a sync-friendly CRUD
//!   API using the `nail-api` domain types. See [`store`] for the threading
//!   model.
//! * [`BlobStore`] — raw RFC 5322 messages, addressed by SHA-256, zstd
//!   compressed, deduplicated and written atomically.
//! * The operation queue ([`Store::enqueue`], [`Store::replay_account`],
//!   [`Store::replay_pending`], [`Replayer`]) — every user write is applied
//!   locally and queued in one transaction, then replayed to the server per
//!   account; it survives crashes. The sync-side merge
//!   ([`Store::merge_messages`]) keeps pending local changes.
//! * Threading ([`thread`]) — incremental thread membership over
//!   Message-ID / References with a subject fallback, threads spanning
//!   folders and accounts, and the JWZ algorithm for reply trees.
//! * Drafts and the outbox ([`Store::save_draft`], [`Store::schedule_send`]),
//!   draft attachments and server copies ([`DraftServerCopy`]).
//! * Store-side search filters ([`MessageFilter`]).
//! * POP3 bookkeeping ([`Store::pop3_state`]).
//! * M1b (schema v4, ADR 0022): collected addresses
//!   ([`Store::suggest_addresses`]), rule runs and their undo information
//!   ([`RuleRunRecord`], [`RuleRunAction`]), MCP clients with hashed tokens,
//!   grants and the audit log ([`hash_mcp_token`], [`McpAuditRecord`]) and
//!   saved searches ([`Store::list_saved_searches`]).
//! * M2a (schema v5, ADR 0029): the keyring ([`Store::upsert_key`],
//!   [`KeyData`] with already-encrypted secret parts), trust decisions,
//!   Autocrypt peer state ([`AutocryptPeerRecord`]), encryption-at-rest
//!   state ([`VaultRecord`]), cached message security
//!   ([`MessageSecurityRecord`]) and JSON settings ([`Store::setting`]).
//! * Encryption-at-rest hooks (M2a WP33, [`seal`]): a [`DataCipher`]
//!   installed with [`Store::set_cipher`] / [`BlobStore::set_cipher`] seals
//!   sensitive columns and blob files; [`Store::convert_fields`],
//!   [`BlobStore::convert`] and [`Store::compact`] serve the migration.
//!
//! A data directory conventionally holds [`DB_FILE_NAME`] and
//! [`BLOB_DIR_NAME`]; [`open_data_dir`] opens both.

mod addresses;
pub mod blob;
mod compose;
mod crypto;
mod error;
mod filter;
mod mail;
mod mcp;
mod pop3;
mod queue;
mod rules;
mod saved_search;
mod schema;
pub mod seal;
pub mod store;
pub mod thread;

use std::path::Path;

pub use blob::{BlobHash, BlobStore};
pub use compose::{DraftServerCopy, SendOutcome};
pub use crypto::{
    AutocryptPeerRecord, KeyData, MessageSecurityRecord, SECURITY_SETTINGS_KEY, SecretKeyData,
    StoredKey, VaultRecord,
};
pub use error::{Error, Result};
pub use filter::{MessageFilter, SizeCmp};
pub use mail::{
    FolderCounts, FolderSyncState, MergeReport, MessageMeta, MessagePage, StoredMessage,
    SyncedMessage,
};
pub use mcp::{McpAuditRecord, hash_mcp_token};
pub use pop3::{Pop3State, Pop3Uid};
pub use queue::{
    Enqueued, FlagChange, MessageSource, OpId, OpState, Operation, QueuedOp, ReplayOutcome,
    ReplayPolicy, ReplayReport, Replayer,
};
pub use rules::{RuleRunAction, RuleRunRecord, RuleRunUndo, RuleState};
pub use schema::SCHEMA_VERSION;
pub use seal::{
    CipherError, DataCipher, FieldCounts, SEALED_COLUMNS, SEALED_FIELD_PREFIX, SEALED_MAGIC,
    SealMode, SealedColumn,
};
pub use store::{BLOB_DIR_NAME, DB_FILE_NAME, Store};
pub use thread::{ThreadPage, ThreadTreeEntry};

/// Opens the metadata database and blob store inside a data directory
/// (`<dir>/nail.sqlite3` and `<dir>/blobs/`), creating them if needed.
pub fn open_data_dir(dir: impl AsRef<Path>) -> Result<(Store, BlobStore)> {
    let dir = dir.as_ref();
    let store = Store::open(dir.join(DB_FILE_NAME))?;
    let blobs = BlobStore::open(dir.join(BLOB_DIR_NAME))?;
    Ok((store, blobs))
}
