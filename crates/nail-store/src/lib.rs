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
//! * M2b (schema v6, ADR 0036): remote-UI users with hashed / referenced
//!   credentials, passkeys, paired devices, sessions (token hashes with
//!   rotation), pairing codes, login throttling, the remote audit log
//!   ([`RemoteAuditRecord`]) and the remote-content allow-lists
//!   ([`Store::remote_content_allowed_by`]).
//! * M2c (schema v7, ADR 0043): message body state for sync policies
//!   ([`Store::set_body_state`]), snooze wake times ([`SnoozeRecord`]), the
//!   junk classifier with hashed tokens ([`Store::train_junk`],
//!   [`hash_junk_token`]) and verdicts, notification settings with
//!   per-account / per-folder overrides ([`Store::notification_settings`])
//!   and the Sieve state per account ([`SieveRecord`], vacation settings).
//! * Encryption-at-rest hooks (M2a WP33, [`seal`]): a [`DataCipher`]
//!   installed with [`Store::set_cipher`] / [`BlobStore::set_cipher`] seals
//!   sensitive columns and blob files; [`Store::convert_fields`],
//!   [`BlobStore::convert`] and [`Store::compact`] serve the migration.
//!
//! * M2c WP53: body-less messages of "headers only" / "recent N days"
//!   folders and on-demand bodies ([`Store::merge_with_body_state`],
//!   [`Store::attach_body`], [`Store::detach_body`],
//!   [`Store::body_fetch_targets`], [`Store::bodies_before`]); the
//!   single-instance lock of a data directory ([`DataDirLock`]); the MCP
//!   settings ([`Store::mcp_settings`]).
//! * M3a (schema v10, ADRs 0058 / 0061): calendars, contacts and
//!   tasks ([`pim`]): sources and collections, items with their raw
//!   iCalendar / vCard (sealed), user writes through the PIM queue
//!   ([`Store::save_pim_item`], [`Store::replay_pim_source`]), server
//!   merges with conflict copies ([`Store::apply_server_item`]), the
//!   instance cache ([`Store::list_event_instances`]), reminders, duplicate
//!   detection and merging ([`Store::merge_contacts`]), the Collected book
//!   and address autocomplete ([`Store::suggest_recipients`]).
//! * M2d (schema v9, ADR 0050): installed plug-ins ([`PluginRecord`],
//!   [`Store::install_plugin`]), their grants, settings, key-value storage
//!   with quotas ([`Store::plugin_storage_set`]) and bounded logs
//!   ([`Store::append_plugin_log`]); storage values and log messages are
//!   sealed at rest.
//!
//! * M3b (schema v11, ADR 0065): remote ids of API-account messages
//!   ([`Store::message_by_remote_id`]), the scheduling state of
//!   invitations ([`scheduling`]: `SEQUENCE` / `DTSTAMP` / answer per
//!   event and address, received and sent iTIP messages, settings) and
//!   Gmail accounts ([`gmail`]: `historyId`, labels ↔ tags / folders,
//!   per-message labels and category, the label operation queue).
//!
//! * M4a (schema v14, ADR 0079): confirmations of writes by MCP clients
//!   and plug-ins ([`Store::insert_confirmation`], decided once, view and
//!   payload sealed, history pruned), action policies
//!   ([`Store::set_action_policy`]; `allow` refused for destructive kinds),
//!   MCP rate limits and idempotency keys ([`Store::set_mcp_rate_limits`],
//!   [`Store::mcp_idempotent_result`]), plug-in job state, hook statistics
//!   and CPU time ([`Store::record_plugin_job_run`],
//!   [`Store::record_plugin_hook_call`], [`Store::plugin_cpu_last_hour`]).
//!
//! A data directory conventionally holds [`DB_FILE_NAME`] and
//! [`BLOB_DIR_NAME`]; [`open_data_dir`] opens both.

mod addresses;
mod agents;
pub mod blob;
mod bodies;
mod compose;
mod crypto;
mod error;
pub mod exchange;
pub mod exchange_sync;
mod filter;
pub mod gmail;
pub mod lock;
mod mail;
mod mcp;
pub mod pim;
mod plugin_activity;
mod plugins;
mod pop3;
mod queue;
mod remote;
mod rules;
mod saved_search;
pub mod scheduling;
mod schema;
pub mod seal;
pub mod store;
pub mod thread;
mod triage;

use std::path::Path;

pub use agents::InsertedConfirmation;
pub use blob::{BlobHash, BlobStore};
pub use bodies::{BodyFetchTarget, StoredBody};
pub use compose::{DraftServerCopy, SendOutcome, draft_revision};
pub use crypto::{
    AutocryptPeerRecord, KeyData, MessageSecurityRecord, SECURITY_SETTINGS_KEY, SecretKeyData,
    StoredKey, VaultRecord,
};
pub use error::{Error, Result};
pub use filter::{MessageFilter, SizeCmp};
pub use lock::{DataDirLock, LOCK_FILE, LockError, LockHolder};
pub use mail::{
    FolderCounts, FolderSyncState, MergeReport, MessageMeta, MessagePage, StoredMessage,
    SyncedMessage,
};
pub use mcp::{McpAuditRecord, hash_mcp_token};
pub use plugin_activity::{MAX_ACTIVITY_ERROR, PluginJobState};
pub use plugins::{
    PluginInstall, PluginLogLine, PluginRecord, check_grants as check_plugin_grants,
};
pub use pop3::{Pop3State, Pop3Uid};
pub use queue::{
    Enqueued, FlagChange, MessageSource, OpId, OpState, Operation, QueuedOp, ReplayOutcome,
    ReplayPolicy, ReplayReport, Replayer,
};
pub use remote::{
    NewPairingCode, NewPasskey, NewRemoteDevice, NewRemoteSession, REMOTE_UI_CERTIFICATE_KEY,
    REMOTE_UI_SETTINGS_KEY, RemoteAuditRecord, RemoteCredentials, StoredPasskey, ThrottleRecord,
    hash_remote_secret,
};
pub use rules::{RuleRunAction, RuleRunRecord, RuleRunUndo, RuleState};
pub use schema::SCHEMA_VERSION;
pub use seal::{
    CipherError, DataCipher, FieldCounts, SEALED_COLUMNS, SEALED_FIELD_PREFIX, SEALED_MAGIC,
    SealMode, SealedColumn,
};
pub use store::{BLOB_DIR_NAME, DB_FILE_NAME, Store};
pub use thread::{ThreadPage, ThreadTreeEntry};
pub use triage::{
    JUNK_TOKEN_LIMIT, JUNK_TRAINING_ROWS, JUNK_TRAINING_WITH_TOKENS, JunkStats, MCP_SETTINGS_KEY,
    NOTIFICATION_SETTINGS_KEY, SieveLocation, SieveRecord, SieveUpload, SnoozeRecord, TokenCounts,
    TrainOutcome, hash_junk_token, hash_sieve_script,
};

/// Opens the metadata database and blob store inside a data directory
/// (`<dir>/nail.sqlite3` and `<dir>/blobs/`), creating them if needed.
pub fn open_data_dir(dir: impl AsRef<Path>) -> Result<(Store, BlobStore)> {
    let dir = dir.as_ref();
    let store = Store::open(dir.join(DB_FILE_NAME))?;
    let blobs = BlobStore::open(dir.join(BLOB_DIR_NAME))?;
    Ok((store, blobs))
}
