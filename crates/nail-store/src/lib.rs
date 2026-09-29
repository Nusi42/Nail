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
//! * Drafts and the outbox ([`Store::save_draft`], [`Store::schedule_send`]).
//! * Store-side search filters ([`MessageFilter`]).
//!
//! A data directory conventionally holds [`DB_FILE_NAME`] and
//! [`BLOB_DIR_NAME`]; [`open_data_dir`] opens both.

pub mod blob;
mod compose;
mod error;
mod filter;
mod mail;
mod queue;
mod schema;
pub mod store;
pub mod thread;

use std::path::Path;

pub use blob::{BlobHash, BlobStore};
pub use compose::SendOutcome;
pub use error::{Error, Result};
pub use filter::{MessageFilter, SizeCmp};
pub use mail::{
    FolderCounts, FolderSyncState, MergeReport, MessageMeta, MessagePage, StoredMessage,
    SyncedMessage,
};
pub use queue::{
    Enqueued, FlagChange, MessageSource, OpId, OpState, Operation, QueuedOp, ReplayOutcome,
    ReplayPolicy, ReplayReport, Replayer,
};
pub use schema::SCHEMA_VERSION;
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
