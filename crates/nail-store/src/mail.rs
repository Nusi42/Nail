//! Accounts, folders and messages: the sync-facing CRUD API.
//!
//! These methods write directly and are meant for the **sync engine**, which
//! mirrors server state into the store. User actions (flag, move, delete, …)
//! must go through the operation queue instead ([`Store::enqueue`]) so they
//! are applied locally *and* replayed to the server.
//!
//! Folder `unread` / `total` counts are never stored; they are computed from
//! the `messages` table when folders are read, so they cannot drift.

use nail_api::{
    Account, AccountId, AccountKind, Address, Flags, Folder, FolderId, FolderRole, MessageId,
    MessageSummary,
};
use rusqlite::types::Type;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::blob::{BlobHash, BlobStore};
use crate::error::{Error, Result};
use crate::store::Store;

/// IMAP synchronization state of a folder. All fields are `None` until the
/// folder was synced once (and `highestmodseq` stays `None` on servers
/// without CONDSTORE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FolderSyncState {
    /// `UIDVALIDITY` of the mailbox; a change invalidates all stored UIDs.
    pub uidvalidity: Option<u32>,
    /// `UIDNEXT`: the next UID the server will assign.
    pub uidnext: Option<u32>,
    /// `HIGHESTMODSEQ` (RFC 7162), when the server supports CONDSTORE.
    pub highestmodseq: Option<u64>,
}

/// A message as stored: the summary shown to clients plus sync metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMessage {
    /// Everything a message list shows.
    pub summary: MessageSummary,
    /// IMAP UID within the folder; `None` for local-only messages.
    pub uid: Option<u32>,
    /// Hash of the raw RFC 5322 message in the blob store, `None` while only
    /// headers are known.
    pub blob_hash: Option<BlobHash>,
    /// Thread grouping key (JWZ root Message-ID or a provider thread id).
    pub thread_key: Option<String>,
}

impl StoredMessage {
    /// A stored message without sync metadata.
    pub fn new(summary: MessageSummary) -> Self {
        Self {
            summary,
            uid: None,
            blob_hash: None,
            thread_key: None,
        }
    }
}

/// One page of a folder's messages, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePage {
    /// Total number of messages in the folder.
    pub total: u32,
    /// The requested page.
    pub messages: Vec<MessageSummary>,
}

/// Message counts of one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderCounts {
    /// The folder.
    pub folder_id: FolderId,
    /// Messages without `\Seen`.
    pub unread: u32,
    /// All messages.
    pub total: u32,
}

// ---------------------------------------------------------------------------
// Column helpers

/// Serializes a unit-like enum to its serde name (e.g. `FolderRole::Inbox` → `"inbox"`).
fn enum_to_sql<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(Error::InvalidData(format!(
            "expected a string enum, got {other}"
        ))),
    }
}

fn conversion_error(
    idx: usize,
    err: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(err))
}

fn enum_col<T: DeserializeOwned>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let s: String = row.get(idx)?;
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| conversion_error(idx, e))
}

fn json_col<T: DeserializeOwned>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let s: String = row.get(idx)?;
    serde_json::from_str(&s).map_err(|e| conversion_error(idx, e))
}

fn hash_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<BlobHash>> {
    let s: Option<String> = row.get(idx)?;
    s.map(|s| BlobHash::parse(&s).map_err(|e| conversion_error(idx, e)))
        .transpose()
}

fn modseq_to_sql(modseq: Option<u64>) -> Result<Option<i64>> {
    modseq
        .map(|m| {
            i64::try_from(m).map_err(|_| Error::InvalidData(format!("HIGHESTMODSEQ {m} too large")))
        })
        .transpose()
}

const ACCOUNT_COLS: &str = "id, name, email, kind";

fn account_from_row(row: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: AccountId(row.get(0)?),
        name: row.get(1)?,
        email: row.get(2)?,
        kind: enum_col::<AccountKind>(row, 3)?,
    })
}

const FOLDER_COLS: &str = "f.id, f.account_id, f.parent_id, f.name, f.path, f.role,
    (SELECT COUNT(*) FROM messages m WHERE m.folder_id = f.id AND m.seen = 0),
    (SELECT COUNT(*) FROM messages m WHERE m.folder_id = f.id)";

fn folder_from_row(row: &Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: FolderId(row.get(0)?),
        account_id: AccountId(row.get(1)?),
        parent_id: row.get::<_, Option<String>>(2)?.map(FolderId),
        name: row.get(3)?,
        path: row.get(4)?,
        role: enum_col::<FolderRole>(row, 5)?,
        unread: row.get(6)?,
        total: row.get(7)?,
    })
}

const MESSAGE_COLS: &str = "m.id, m.folder_id, m.account_id, m.subject, m.from_name, m.from_email,
    m.to_json, m.date, m.seen, m.flagged, m.answered, m.draft, m.has_attachments, m.preview,
    m.uid, m.blob_hash, m.thread_key";

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    let from_email: Option<String> = row.get(5)?;
    Ok(MessageSummary {
        id: MessageId(row.get(0)?),
        folder_id: FolderId(row.get(1)?),
        account_id: AccountId(row.get(2)?),
        subject: row.get(3)?,
        from: from_email
            .map(|email| -> rusqlite::Result<Address> {
                Ok(Address {
                    name: row.get(4)?,
                    email,
                })
            })
            .transpose()?,
        to: json_col(row, 6)?,
        date: row.get(7)?,
        flags: Flags {
            seen: row.get(8)?,
            flagged: row.get(9)?,
            answered: row.get(10)?,
            draft: row.get(11)?,
        },
        has_attachments: row.get(12)?,
        preview: row.get(13)?,
        // Thread ids, tags and virtual-account duplicates arrive with the
        // M1a store schema v2 (WP13).
        thread_id: None,
        tags: Vec::new(),
        duplicates: Vec::new(),
    })
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
        summary: summary_from_row(row)?,
        uid: row.get(14)?,
        blob_hash: hash_col(row, 15)?,
        thread_key: row.get(16)?,
    })
}

/// Orders folders so every parent precedes its children, siblings by path.
/// Folders whose parent is missing are treated as roots.
fn parents_first(folders: Vec<Folder>) -> Vec<Folder> {
    use std::collections::{HashMap, HashSet};

    let ids: HashSet<FolderId> = folders.iter().map(|f| f.id.clone()).collect();
    let mut children: HashMap<Option<FolderId>, Vec<Folder>> = HashMap::new();
    for folder in folders {
        let parent = folder.parent_id.clone().filter(|p| ids.contains(p));
        children.entry(parent).or_default().push(folder);
    }
    let mut out = Vec::with_capacity(ids.len());
    // Iterative DFS; `folders` came sorted by path, so siblings stay sorted.
    let mut stack: Vec<Folder> = children.remove(&None).unwrap_or_default();
    stack.reverse();
    while let Some(folder) = stack.pop() {
        if let Some(mut kids) = children.remove(&Some(folder.id.clone())) {
            kids.reverse();
            stack.extend(kids);
        }
        out.push(folder);
    }
    // Cycles (a → b → a) are unreachable from the roots; append them as-is.
    for (_, rest) in children {
        out.extend(rest);
    }
    out
}

// ---------------------------------------------------------------------------
// Transaction-level helpers shared with the operation queue.

pub(crate) fn message_in_tx(tx: &Transaction<'_>, id: &MessageId) -> Result<StoredMessage> {
    tx.query_row(
        &format!("SELECT {MESSAGE_COLS} FROM messages m WHERE m.id = ?1"),
        [id.as_str()],
        message_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("message", id))
}

pub(crate) fn set_flags_in_tx(tx: &Transaction<'_>, id: &MessageId, flags: Flags) -> Result<()> {
    let changed = tx.execute(
        "UPDATE messages SET seen = ?2, flagged = ?3, answered = ?4, draft = ?5 WHERE id = ?1",
        params![
            id.as_str(),
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft
        ],
    )?;
    if changed == 0 {
        return Err(Error::not_found("message", id));
    }
    Ok(())
}

fn folder_account_in_tx(tx: &Transaction<'_>, folder: &FolderId) -> Result<AccountId> {
    tx.query_row(
        "SELECT account_id FROM folders WHERE id = ?1",
        [folder.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(AccountId)
    .ok_or_else(|| Error::not_found("folder", folder))
}

fn upsert_message_in_tx(tx: &Transaction<'_>, msg: &StoredMessage) -> Result<()> {
    let s = &msg.summary;
    let folder_account = folder_account_in_tx(tx, &s.folder_id)?;
    if folder_account != s.account_id {
        return Err(Error::InvalidData(format!(
            "message {} claims account {} but folder {} belongs to {}",
            s.id, s.account_id, s.folder_id, folder_account
        )));
    }
    let changed = tx.execute(
        "INSERT INTO messages (id, account_id, folder_id, uid, subject, from_name, from_email,
             to_json, date, date_sort, seen, flagged, answered, draft, has_attachments, preview,
             blob_hash, thread_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE(unixepoch(?9), 0), ?10, ?11, ?12,
             ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT (id) DO UPDATE SET
             folder_id = excluded.folder_id, uid = excluded.uid, subject = excluded.subject,
             from_name = excluded.from_name, from_email = excluded.from_email,
             to_json = excluded.to_json, date = excluded.date, date_sort = excluded.date_sort,
             seen = excluded.seen, flagged = excluded.flagged, answered = excluded.answered,
             draft = excluded.draft, has_attachments = excluded.has_attachments,
             preview = excluded.preview, blob_hash = excluded.blob_hash,
             thread_key = excluded.thread_key
         WHERE messages.account_id = excluded.account_id",
        params![
            s.id.as_str(),
            s.account_id.as_str(),
            s.folder_id.as_str(),
            msg.uid,
            s.subject,
            s.from.as_ref().and_then(|a| a.name.as_deref()),
            s.from.as_ref().map(|a| a.email.as_str()),
            serde_json::to_string(&s.to)?,
            s.date,
            s.flags.seen,
            s.flags.flagged,
            s.flags.answered,
            s.flags.draft,
            s.has_attachments,
            s.preview,
            msg.blob_hash.as_ref().map(BlobHash::as_str),
            msg.thread_key,
        ],
    )?;
    if changed == 0 {
        return Err(Error::InvalidData(format!(
            "message {} already exists in another account",
            s.id
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API

impl Store {
    // --- accounts ----------------------------------------------------------

    /// Inserts or updates an account (matched by id). Accounts are listed in
    /// the order they were first inserted.
    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        let kind = enum_to_sql(&account.kind)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO accounts (id, name, email, kind) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET
                     name = excluded.name, email = excluded.email, kind = excluded.kind",
                params![account.id.as_str(), account.name, account.email, kind],
            )?;
            Ok(())
        })
    }

    /// One account.
    pub fn get_account(&self, id: &AccountId) -> Result<Account> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?1"),
                [id.as_str()],
                account_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("account", id))
        })
    }

    /// All accounts in insertion order.
    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ACCOUNT_COLS} FROM accounts ORDER BY rowid"
            ))?;
            let rows = stmt.query_map([], account_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Deletes an account with all its folders, messages and queued
    /// operations. Returns whether it existed. Blobs are not touched.
    pub fn delete_account(&self, id: &AccountId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM accounts WHERE id = ?1", [id.as_str()])? > 0)
        })
    }

    // --- folders -----------------------------------------------------------

    /// Inserts or updates a folder (matched by id). `unread` and `total` are
    /// ignored (they are computed) and the sync state is left untouched.
    ///
    /// Fails with [`Error::NotFound`] for an unknown account and with
    /// [`Error::InvalidData`] when the folder would move to another account
    /// or its parent belongs to another account.
    pub fn upsert_folder(&self, folder: &Folder) -> Result<()> {
        let role = enum_to_sql(&folder.role)?;
        self.with_tx(|tx| {
            let account_exists = tx
                .query_row(
                    "SELECT 1 FROM accounts WHERE id = ?1",
                    [folder.account_id.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !account_exists {
                return Err(Error::not_found("account", &folder.account_id));
            }
            if let Some(parent) = &folder.parent_id {
                if *parent == folder.id {
                    return Err(Error::InvalidData(format!(
                        "folder {} is its own parent",
                        folder.id
                    )));
                }
                let parent_account = folder_account_in_tx(tx, parent)?;
                if parent_account != folder.account_id {
                    return Err(Error::InvalidData(format!(
                        "parent {parent} of folder {} belongs to another account",
                        folder.id
                    )));
                }
            }
            let changed = tx.execute(
                "INSERT INTO folders (id, account_id, parent_id, name, path, role)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (id) DO UPDATE SET
                     parent_id = excluded.parent_id, name = excluded.name,
                     path = excluded.path, role = excluded.role
                 WHERE folders.account_id = excluded.account_id",
                params![
                    folder.id.as_str(),
                    folder.account_id.as_str(),
                    folder.parent_id.as_ref().map(FolderId::as_str),
                    folder.name,
                    folder.path,
                    role,
                ],
            )?;
            if changed == 0 {
                return Err(Error::InvalidData(format!(
                    "folder {} already exists in another account",
                    folder.id
                )));
            }
            Ok(())
        })
    }

    /// One folder, with current counts.
    pub fn get_folder(&self, id: &FolderId) -> Result<Folder> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT {FOLDER_COLS} FROM folders f WHERE f.id = ?1"),
                [id.as_str()],
                folder_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("folder", id))
        })
    }

    /// A folder by its server path, if present.
    pub fn find_folder_by_path(&self, account: &AccountId, path: &str) -> Result<Option<Folder>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {FOLDER_COLS} FROM folders f WHERE f.account_id = ?1 AND f.path = ?2"),
                    [account.as_str(), path],
                    folder_from_row,
                )
                .optional()?)
        })
    }

    /// All folders of an account with current counts, parents before
    /// children and siblings ordered by path. Fails with
    /// [`Error::NotFound`] for an unknown account.
    pub fn list_folders(&self, account: &AccountId) -> Result<Vec<Folder>> {
        let folders = self.with_conn(|conn| {
            let exists = conn
                .query_row(
                    "SELECT 1 FROM accounts WHERE id = ?1",
                    [account.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Err(Error::not_found("account", account));
            }
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {FOLDER_COLS} FROM folders f WHERE f.account_id = ?1 ORDER BY f.path, f.id"
            ))?;
            let rows = stmt.query_map([account.as_str()], folder_from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        Ok(parents_first(folders))
    }

    /// Deletes a folder and its messages; child folders become top-level.
    /// Returns whether it existed.
    pub fn delete_folder(&self, id: &FolderId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM folders WHERE id = ?1", [id.as_str()])? > 0)
        })
    }

    /// The folder's IMAP sync state.
    pub fn folder_sync_state(&self, id: &FolderId) -> Result<FolderSyncState> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT uidvalidity, uidnext, highestmodseq FROM folders WHERE id = ?1",
                [id.as_str()],
                |row| {
                    Ok(FolderSyncState {
                        uidvalidity: row.get(0)?,
                        uidnext: row.get(1)?,
                        highestmodseq: row.get::<_, Option<i64>>(2)?.map(|m| m.cast_unsigned()),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::not_found("folder", id))
        })
    }

    /// Replaces the folder's IMAP sync state.
    pub fn set_folder_sync_state(&self, id: &FolderId, state: &FolderSyncState) -> Result<()> {
        let modseq = modseq_to_sql(state.highestmodseq)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE folders SET uidvalidity = ?2, uidnext = ?3, highestmodseq = ?4 WHERE id = ?1",
                params![id.as_str(), state.uidvalidity, state.uidnext, modseq],
            )?;
            if changed == 0 {
                return Err(Error::not_found("folder", id));
            }
            Ok(())
        })
    }

    /// Message counts of every folder of an account.
    pub fn unread_counts(&self, account: &AccountId) -> Result<Vec<FolderCounts>> {
        Ok(self
            .list_folders(account)?
            .into_iter()
            .map(|f| FolderCounts {
                folder_id: f.id,
                unread: f.unread,
                total: f.total,
            })
            .collect())
    }

    // --- messages ----------------------------------------------------------

    /// Inserts or updates one message (matched by id). Moving a message to
    /// another folder of the same account is an upsert with a new
    /// `folder_id`.
    ///
    /// Fails with [`Error::NotFound`] for an unknown folder and with
    /// [`Error::InvalidData`] when the message's account differs from the
    /// folder's account.
    pub fn upsert_message(&self, message: &StoredMessage) -> Result<()> {
        self.with_tx(|tx| upsert_message_in_tx(tx, message))
    }

    /// Upserts many messages in one transaction (all or nothing), which is
    /// much faster than one call per message during sync.
    pub fn upsert_messages(&self, messages: &[StoredMessage]) -> Result<()> {
        self.with_tx(|tx| {
            messages
                .iter()
                .try_for_each(|m| upsert_message_in_tx(tx, m))
        })
    }

    /// One message with sync metadata.
    pub fn get_message(&self, id: &MessageId) -> Result<StoredMessage> {
        self.with_tx(|tx| message_in_tx(tx, id))
    }

    /// The message with this UID in a folder, if present.
    pub fn find_message_by_uid(
        &self,
        folder: &FolderId,
        uid: u32,
    ) -> Result<Option<StoredMessage>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {MESSAGE_COLS} FROM messages m WHERE m.folder_id = ?1 AND m.uid = ?2"),
                    params![folder.as_str(), uid],
                    message_from_row,
                )
                .optional()?)
        })
    }

    /// All `(uid, id)` pairs of a folder's messages that have a UID, by UID.
    pub fn folder_uids(&self, folder: &FolderId) -> Result<Vec<(u32, MessageId)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT uid, id FROM messages WHERE folder_id = ?1 AND uid IS NOT NULL ORDER BY uid",
            )?;
            let rows = stmt.query_map([folder.as_str()], |row| Ok((row.get(0)?, MessageId(row.get(1)?))))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// A page of a folder's messages, newest first (by date, then id for a
    /// stable order), plus the folder's total. Fails with
    /// [`Error::NotFound`] for an unknown folder.
    pub fn list_messages(&self, folder: &FolderId, offset: u32, limit: u32) -> Result<MessagePage> {
        self.with_tx(|tx| {
            let total: Option<u32> = tx
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM messages WHERE folder_id = f.id) FROM folders f WHERE f.id = ?1",
                    [folder.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            let total = total.ok_or_else(|| Error::not_found("folder", folder))?;
            let mut stmt = tx.prepare_cached(&format!(
                "SELECT {MESSAGE_COLS} FROM messages m WHERE m.folder_id = ?1
                 ORDER BY m.date_sort DESC, m.id DESC LIMIT ?2 OFFSET ?3"
            ))?;
            let rows = stmt.query_map(params![folder.as_str(), limit, offset], summary_from_row)?;
            let messages = rows.collect::<rusqlite::Result<_>>()?;
            Ok(MessagePage { total, messages })
        })
    }

    /// Replaces a message's flags with the server's view (sync only; user
    /// flag changes go through [`Store::enqueue`]). Returns the message's
    /// folder so the caller can announce the change.
    pub fn update_flags(&self, id: &MessageId, flags: Flags) -> Result<FolderId> {
        self.with_tx(|tx| {
            set_flags_in_tx(tx, id, flags)?;
            Ok(message_in_tx(tx, id)?.summary.folder_id)
        })
    }

    /// Deletes a message. Returns whether it existed. Its blob is not
    /// touched (see [`Store::blob_is_referenced`]).
    pub fn delete_message(&self, id: &MessageId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM messages WHERE id = ?1", [id.as_str()])? > 0)
        })
    }

    /// Deletes the messages with these UIDs from a folder (expunged on the
    /// server). Returns how many were deleted.
    pub fn delete_messages_by_uid(&self, folder: &FolderId, uids: &[u32]) -> Result<usize> {
        self.with_tx(|tx| {
            let mut stmt =
                tx.prepare_cached("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2")?;
            let mut deleted = 0;
            for uid in uids {
                deleted += stmt.execute(params![folder.as_str(), uid])?;
            }
            Ok(deleted)
        })
    }

    /// Deletes all messages of a folder and resets its sync state, e.g.
    /// after a `UIDVALIDITY` change. Returns how many messages were deleted.
    pub fn clear_folder(&self, folder: &FolderId) -> Result<usize> {
        self.with_tx(|tx| {
            folder_account_in_tx(tx, folder)?;
            let deleted = tx.execute("DELETE FROM messages WHERE folder_id = ?1", [folder.as_str()])?;
            tx.execute(
                "UPDATE folders SET uidvalidity = NULL, uidnext = NULL, highestmodseq = NULL WHERE id = ?1",
                [folder.as_str()],
            )?;
            Ok(deleted)
        })
    }

    /// The distinct blobs the messages of a folder reference. Collect them
    /// before deleting or clearing the folder, then pass them to
    /// [`Store::remove_unreferenced_blobs`].
    pub fn folder_blob_hashes(&self, folder: &FolderId) -> Result<Vec<BlobHash>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT DISTINCT blob_hash FROM messages \
                 WHERE folder_id = ?1 AND blob_hash IS NOT NULL",
            )?;
            let hashes = stmt
                .query_map([folder.as_str()], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            hashes.iter().map(|h| BlobHash::parse(h)).collect()
        })
    }

    /// Deletes those `candidates` from `blobs` that no message references
    /// any more (garbage collection after deleting messages). Returns how
    /// many blob files were removed.
    ///
    /// Each check and removal happens while holding the connection, so no
    /// message row referencing the blob can be written in between. A writer
    /// that stored the blob *before* and writes its row *after* a removal
    /// must re-store the blob (see `nail-core`'s ingest).
    pub fn remove_unreferenced_blobs(
        &self,
        blobs: &BlobStore,
        candidates: &[BlobHash],
    ) -> Result<usize> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare_cached("SELECT 1 FROM messages WHERE blob_hash = ?1 LIMIT 1")?;
            let mut removed = 0;
            for hash in candidates {
                if !stmt.exists([hash.as_str()])? && blobs.remove(hash)? {
                    removed += 1;
                }
            }
            Ok(removed)
        })
    }

    /// Whether any message still references this blob (for garbage
    /// collection of the blob store).
    pub fn blob_is_referenced(&self, hash: &BlobHash) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM messages WHERE blob_hash = ?1 LIMIT 1",
                    [hash.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }
}
