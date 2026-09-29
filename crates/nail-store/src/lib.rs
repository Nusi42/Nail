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
//! * The operation queue ([`Store::enqueue`], [`Store::replay_pending`],
//!   [`Replayer`]) — every user write is applied locally and queued in one
//!   transaction, then replayed to the server; it survives crashes.
//!
//! A data directory conventionally holds [`DB_FILE_NAME`] and
//! [`BLOB_DIR_NAME`]; [`open_data_dir`] opens both.

pub mod blob;
mod error;
mod mail;
mod queue;
mod schema;
pub mod store;

use std::path::Path;

pub use blob::{BlobHash, BlobStore};
pub use error::{Error, Result};
pub use mail::{FolderCounts, FolderSyncState, MessagePage, StoredMessage};
pub use queue::{
    Enqueued, FlagChange, OpId, OpState, Operation, QueuedOp, ReplayOutcome, ReplayPolicy,
    ReplayReport, Replayer,
};
pub use schema::SCHEMA_VERSION;
pub use store::{BLOB_DIR_NAME, DB_FILE_NAME, Store};

/// Opens the metadata database and blob store inside a data directory
/// (`<dir>/nail.sqlite3` and `<dir>/blobs/`), creating them if needed.
pub fn open_data_dir(dir: impl AsRef<Path>) -> Result<(Store, BlobStore)> {
    let dir = dir.as_ref();
    let store = Store::open(dir.join(DB_FILE_NAME))?;
    let blobs = BlobStore::open(dir.join(BLOB_DIR_NAME))?;
    Ok((store, blobs))
}
