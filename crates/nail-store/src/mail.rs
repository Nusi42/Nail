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
    MessageLocation, MessageSummary, ThreadId,
};
use std::collections::HashMap;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::blob::{BlobHash, BlobStore};
use crate::error::{Error, Result};
use crate::queue::Operation;
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
///
/// In `summary`, `thread_id` is ignored on writes (the store threads
/// messages itself) and `tags` are the server's keywords; `duplicates` is
/// ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMessage {
    /// Everything a message list shows.
    pub summary: MessageSummary,
    /// IMAP UID within the folder; `None` for local-only messages.
    pub uid: Option<u32>,
    /// Hash of the raw RFC 5322 message in the blob store, `None` while only
    /// headers are known.
    pub blob_hash: Option<BlobHash>,
    /// M0 thread grouping key (root of the reference chain). Only used for
    /// threading when no [`MessageMeta`] was ever given for the message.
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

/// Headers the store threads by, plus the raw size (for `size>` searches).
/// Message-IDs may be given with or without angle brackets.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MessageMeta {
    /// `Message-ID`.
    pub message_id: Option<String>,
    /// `In-Reply-To` ids.
    pub in_reply_to: Vec<String>,
    /// `References` ids, oldest first.
    pub references: Vec<String>,
    /// Size of the raw message in bytes.
    pub size: Option<u64>,
}

/// A message as reported by a server, for [`Store::merge_messages`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedMessage {
    /// Summary and sync metadata.
    pub message: StoredMessage,
    /// Threading headers and size; `None` keeps what is stored.
    pub meta: Option<MessageMeta>,
}

/// What [`Store::merge_messages`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Messages written (including `kept_local`).
    pub written: usize,
    /// Messages written with pending local flag / tag changes re-applied.
    pub kept_local: Vec<MessageId>,
    /// Messages not written because a move or delete is pending for them.
    pub skipped: Vec<MessageId>,
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

pub(crate) const MESSAGE_COLS: &str = "m.id, m.folder_id, m.account_id, m.subject, m.from_name,
    m.from_email, m.to_json, m.date, m.seen, m.flagged, m.answered, m.draft, m.has_attachments,
    nail_open('messages.preview', m.preview), m.uid, m.blob_hash, m.thread_key, m.thread_id,
    m.body_state = 'headers-only'";

/// Number of columns in [`MESSAGE_COLS`]; extra columns selected after it
/// start at this index.
pub(crate) const MESSAGE_COL_COUNT: usize = 19;

/// Reads a summary from a row selected with [`MESSAGE_COLS`]. `tags` stay
/// empty; [`attach_tags`] fills them.
pub(crate) fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    let from_email: Option<String> = row.get(5)?;
    Ok(MessageSummary {
        headers_only: row.get(18)?,
        gmail: None,
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
        thread_id: row.get::<_, Option<String>>(17)?.map(ThreadId),
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
// Transaction-level helpers shared with the operation queue and threading.

/// Fills `tags` (sorted) of each summary from `message_tags`.
pub(crate) fn attach_tags(
    conn: &Connection,
    mut messages: Vec<MessageSummary>,
) -> Result<Vec<MessageSummary>> {
    for m in &mut messages {
        m.tags = tags_in_tx(conn, &m.id)?;
        // Gmail data (M3b): labels, category, thread.
        m.gmail = crate::gmail::gmail_info_in(conn, &m.id)?;
    }
    Ok(messages)
}

/// Collapses copies of the same mail (same `Message-ID`, e.g. a mailing
/// list message received in two accounts) into the first one, listing the
/// others in its `duplicates`. Messages without a Message-ID are kept.
pub(crate) fn collapse_duplicates(
    messages: Vec<(MessageSummary, Option<String>)>,
) -> Vec<MessageSummary> {
    let mut first_by_mid: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<MessageSummary> = Vec::with_capacity(messages.len());
    for (summary, mid) in messages {
        if let Some(mid) = mid {
            if let Some(&i) = first_by_mid.get(&mid) {
                out[i].duplicates.push(MessageLocation {
                    message_id: summary.id,
                    account_id: summary.account_id,
                    folder_id: summary.folder_id,
                });
                continue;
            }
            first_by_mid.insert(mid, out.len());
        }
        out.push(summary);
    }
    out
}

pub(crate) fn message_in_tx(tx: &Transaction<'_>, id: &MessageId) -> Result<StoredMessage> {
    let mut message = tx
        .query_row(
            &format!("SELECT {MESSAGE_COLS} FROM messages m WHERE m.id = ?1"),
            [id.as_str()],
            message_from_row,
        )
        .optional()?
        .ok_or_else(|| Error::not_found("message", id))?;
    message.summary.tags = tags_in_tx(tx, id)?;
    Ok(message)
}

pub(crate) fn tags_in_tx(conn: &Connection, id: &MessageId) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare_cached("SELECT tag FROM message_tags WHERE message_id = ?1 ORDER BY tag")?;
    let tags = stmt
        .query_map([id.as_str()], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(tags)
}

/// Replaces a message's tags (empty tags are ignored).
pub(crate) fn set_tags_in_tx(tx: &Transaction<'_>, id: &MessageId, tags: &[String]) -> Result<()> {
    tx.execute(
        "DELETE FROM message_tags WHERE message_id = ?1",
        [id.as_str()],
    )?;
    let mut stmt =
        tx.prepare_cached("INSERT OR IGNORE INTO message_tags (message_id, tag) VALUES (?1, ?2)")?;
    for tag in tags.iter().filter(|t| !t.is_empty()) {
        stmt.execute(params![id.as_str(), tag])?;
    }
    Ok(())
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

pub(crate) fn folder_account_in_tx(tx: &Transaction<'_>, folder: &FolderId) -> Result<AccountId> {
    tx.query_row(
        "SELECT account_id FROM folders WHERE id = ?1",
        [folder.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(AccountId)
    .ok_or_else(|| Error::not_found("folder", folder))
}

/// Normalizes a Message-ID: trims whitespace and angle brackets; `None`
/// when nothing is left.
pub(crate) fn normalize_msgid(id: &str) -> Option<String> {
    let id = id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    (!id.is_empty()).then(|| id.to_owned())
}

fn normalize_msgids(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids.iter().filter_map(|i| normalize_msgid(i)) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// What the sync-side merge did with one incoming message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeOutcome {
    /// Written as the server reported it.
    Written,
    /// Written with pending local flag / tag changes re-applied on top.
    Adjusted,
    /// Not written: a pending move or delete owns the message.
    Skipped,
}

pub(crate) fn upsert_message_in_tx(
    tx: &Transaction<'_>,
    msg: &StoredMessage,
    meta: Option<&MessageMeta>,
) -> Result<MergeOutcome> {
    let s = &msg.summary;
    let folder_account = folder_account_in_tx(tx, &s.folder_id)?;
    if folder_account != s.account_id {
        return Err(Error::InvalidData(format!(
            "message {} claims account {} but folder {} belongs to {}",
            s.id, s.account_id, s.folder_id, folder_account
        )));
    }

    // Sync-side merge: pending user operations win over the server's view
    // until they are replayed.
    let pending = crate::queue::pending_ops_for_message_in_tx(tx, &s.id)?;
    if pending.iter().any(Operation::takes_message_away) {
        tracing::debug!(message = %s.id, "sync update skipped: a move / delete is pending");
        return Ok(MergeOutcome::Skipped);
    }
    let mut flags = s.flags;
    let mut tags = s.tags.clone();
    let mut outcome = MergeOutcome::Written;
    for op in &pending {
        if let Operation::UpdateFlags { change, .. } = op {
            flags = change.apply_to(flags);
            change.apply_tags(&mut tags);
            outcome = MergeOutcome::Adjusted;
        }
    }

    let changed = tx.execute(
        "INSERT INTO messages (id, account_id, folder_id, uid, subject, from_name, from_email,
             to_json, date, date_sort, seen, flagged, answered, draft, has_attachments, preview,
             blob_hash, thread_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE(unixepoch(?9), 0), ?10, ?11, ?12,
             ?13, ?14, nail_seal('messages.preview', ?15), ?16, ?17)
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
            flags.seen,
            flags.flagged,
            flags.answered,
            flags.draft,
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
    if let Some(meta) = meta {
        let size = meta
            .size
            .map(|n| {
                i64::try_from(n).map_err(|_| Error::InvalidData(format!("size {n} too large")))
            })
            .transpose()?;
        tx.execute(
            "UPDATE messages SET message_id = ?2, in_reply_to_json = ?3, references_json = ?4,
                 size = ?5
             WHERE id = ?1",
            params![
                s.id.as_str(),
                meta.message_id.as_deref().and_then(normalize_msgid),
                serde_json::to_string(&normalize_msgids(&meta.in_reply_to))?,
                serde_json::to_string(&normalize_msgids(&meta.references))?,
                size,
            ],
        )?;
    }
    crate::thread::set_subject_key(tx, s.id.as_str(), &s.subject)?;
    tags.sort();
    tags.dedup();
    set_tags_in_tx(tx, &s.id, &tags)?;
    crate::thread::assign_thread(tx, s.id.as_str())?;
    Ok(outcome)
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

    /// Inserts or updates one message (matched by id) as reported by the
    /// server; see [`Store::merge_messages`] for how pending user
    /// operations are kept. Moving a message to another folder of the same
    /// account is an upsert with a new `folder_id`.
    ///
    /// Header columns (Message-ID, references, size) are left as they are;
    /// use [`Store::merge_messages`] with a [`MessageMeta`] to set them.
    ///
    /// Fails with [`Error::NotFound`] for an unknown folder and with
    /// [`Error::InvalidData`] when the message's account differs from the
    /// folder's account.
    pub fn upsert_message(&self, message: &StoredMessage) -> Result<()> {
        self.with_tx(|tx| upsert_message_in_tx(tx, message, None).map(drop))
    }

    /// Upserts many messages in one transaction (all or nothing), which is
    /// much faster than one call per message during sync. Same rules as
    /// [`Store::upsert_message`].
    pub fn upsert_messages(&self, messages: &[StoredMessage]) -> Result<()> {
        self.with_tx(|tx| {
            messages
                .iter()
                .try_for_each(|m| upsert_message_in_tx(tx, m, None).map(drop))
        })
    }

    /// The sync-side merge: writes the server's view of `messages` in one
    /// transaction (all or nothing) while keeping the user's pending
    /// changes:
    ///
    /// * a message with a pending flag / tag operation gets the server's
    ///   flags and tags **with the pending changes re-applied on top**, so
    ///   the UI keeps showing what the user did until the replay lands
    ///   (and the server's other changes still arrive);
    /// * a message with a pending move or delete is not written at all.
    ///
    /// Each message is (re)threaded. `meta` (when given) sets the
    /// threading headers and the size.
    pub fn merge_messages(&self, messages: &[SyncedMessage]) -> Result<MergeReport> {
        self.with_tx(|tx| {
            let mut report = MergeReport::default();
            for m in messages {
                match upsert_message_in_tx(tx, &m.message, m.meta.as_ref())? {
                    MergeOutcome::Written => report.written += 1,
                    MergeOutcome::Adjusted => {
                        report.written += 1;
                        report.kept_local.push(m.message.summary.id.clone());
                    }
                    MergeOutcome::Skipped => report.skipped.push(m.message.summary.id.clone()),
                }
            }
            Ok(report)
        })
    }

    /// The stored threading headers and size of a message.
    pub fn message_meta(&self, id: &MessageId) -> Result<MessageMeta> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT message_id, in_reply_to_json, references_json, size FROM messages WHERE id = ?1",
                [id.as_str()],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| Error::not_found("message", id))
            .and_then(|(message_id, irt, refs, size)| {
                Ok(MessageMeta {
                    message_id,
                    in_reply_to: serde_json::from_str(&irt)?,
                    references: serde_json::from_str(&refs)?,
                    size: size.map(i64::cast_unsigned),
                })
            })
        })
    }

    /// Messages whose headers were never recorded (written by the M0 sync
    /// or migrated from schema v1) and that have a blob, so the caller can
    /// parse the blob and backfill them with [`Store::merge_messages`].
    pub fn messages_missing_meta(&self, limit: u32) -> Result<Vec<(MessageId, BlobHash)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, blob_hash FROM messages
                 WHERE message_id IS NULL AND size IS NULL AND blob_hash IS NOT NULL
                 ORDER BY rowid LIMIT ?1",
            )?;
            let rows = stmt
                .query_map([limit], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(id, hash)| Ok((MessageId(id), BlobHash::parse(&hash)?)))
                .collect()
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
            let found = conn
                .query_row(
                    &format!("SELECT {MESSAGE_COLS} FROM messages m WHERE m.folder_id = ?1 AND m.uid = ?2"),
                    params![folder.as_str(), uid],
                    message_from_row,
                )
                .optional()?;
            found
                .map(|mut m| {
                    m.summary.tags = tags_in_tx(conn, &m.summary.id)?;
                    Ok(m)
                })
                .transpose()
        })
    }

    /// Messages with this `Message-ID` (angle brackets optional), in any
    /// folder or account, oldest first.
    pub fn find_messages_by_message_id(&self, message_id: &str) -> Result<Vec<MessageSummary>> {
        let Some(mid) = normalize_msgid(message_id) else {
            return Ok(Vec::new());
        };
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {MESSAGE_COLS} FROM messages m WHERE m.message_id = ?1
                 ORDER BY m.date_sort, m.id"
            ))?;
            let rows = stmt
                .query_map([mid], summary_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            attach_tags(conn, rows)
        })
    }

    /// Sets (or clears) a message's UID, e.g. after a replayed move learned
    /// the new UID from `COPYUID`.
    pub fn set_message_uid(&self, id: &MessageId, uid: Option<u32>) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE messages SET uid = ?2 WHERE id = ?1",
                params![id.as_str(), uid],
            )?;
            if changed == 0 {
                return Err(Error::not_found("message", id));
            }
            Ok(())
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
            let messages = attach_tags(tx, rows.collect::<rusqlite::Result<_>>()?)?;
            Ok(MessagePage { total, messages })
        })
    }

    /// A page of the messages in several folders (the member folders of a
    /// virtual folder), newest first. Copies of one mail (same
    /// `Message-ID`) are collapsed into the newest copy's row with the
    /// others in `duplicates`; `total` counts collapsed rows.
    pub fn list_messages_in(
        &self,
        folders: &[FolderId],
        offset: u32,
        limit: u32,
    ) -> Result<MessagePage> {
        let folder_json =
            serde_json::to_string(&folders.iter().map(FolderId::as_str).collect::<Vec<_>>())?;
        self.with_tx(|tx| {
            // One representative per mail: the newest copy.
            const REPS: &str = "WITH scoped AS (
                    SELECT m.id, m.date_sort, COALESCE(m.message_id, '#' || m.id) AS mail,
                           ROW_NUMBER() OVER (
                               PARTITION BY COALESCE(m.message_id, '#' || m.id)
                               ORDER BY m.date_sort DESC, m.id DESC) AS n
                    FROM messages m WHERE m.folder_id IN (SELECT value FROM json_each(?1)))";
            let total: u32 = tx.query_row(
                &format!("{REPS} SELECT COUNT(*) FROM scoped WHERE n = 1"),
                [&folder_json],
                |r| r.get(0),
            )?;
            let page: Vec<(MessageSummary, String)> = {
                let mut stmt = tx.prepare_cached(&format!(
                    "{REPS} SELECT {MESSAGE_COLS}, s.mail FROM scoped s JOIN messages m ON m.id = s.id
                     WHERE s.n = 1 ORDER BY s.date_sort DESC, s.id DESC LIMIT ?2 OFFSET ?3"
                ))?;
                stmt.query_map(params![folder_json, limit, offset], |r| {
                    Ok((summary_from_row(r)?, r.get(MESSAGE_COL_COUNT)?))
                })?
                .collect::<rusqlite::Result<_>>()?
            };
            let mut messages = Vec::with_capacity(page.len());
            let mut dups = tx.prepare_cached(
                "SELECT id, account_id, folder_id FROM messages
                 WHERE message_id = ?1 AND id <> ?2
                   AND folder_id IN (SELECT value FROM json_each(?3))
                 ORDER BY date_sort DESC, id DESC",
            )?;
            for (mut summary, mail) in page {
                if !mail.starts_with('#') {
                    summary.duplicates = dups
                        .query_map(params![mail, summary.id.as_str(), folder_json], |r| {
                            Ok(MessageLocation {
                                message_id: MessageId(r.get(0)?),
                                account_id: AccountId(r.get(1)?),
                                folder_id: FolderId(r.get(2)?),
                            })
                        })?
                        .collect::<rusqlite::Result<_>>()?;
                }
                messages.push(summary);
            }
            let messages = attach_tags(tx, messages)?;
            Ok(MessagePage { total, messages })
        })
    }

    /// Summaries of the given messages in the given order; unknown ids are
    /// skipped.
    pub fn message_summaries(&self, ids: &[MessageId]) -> Result<Vec<MessageSummary>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {MESSAGE_COLS} FROM messages m WHERE m.id = ?1"
            ))?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(s) = stmt.query_row([id.as_str()], summary_from_row).optional()? {
                    out.push(s);
                }
            }
            attach_tags(conn, out)
        })
    }

    /// Replaces a message's flags with the server's view (sync only; user
    /// flag changes go through [`Store::enqueue`]). Flag changes of pending
    /// operations are re-applied on top (see [`Store::merge_messages`]).
    /// Returns the message's folder so the caller can announce the change.
    pub fn update_flags(&self, id: &MessageId, flags: Flags) -> Result<FolderId> {
        self.with_tx(|tx| {
            let mut flags = flags;
            for op in crate::queue::pending_ops_for_message_in_tx(tx, id)? {
                if let Operation::UpdateFlags { change, .. } = op {
                    flags = change.apply_to(flags);
                }
            }
            set_flags_in_tx(tx, id, flags)?;
            Ok(message_in_tx(tx, id)?.summary.folder_id)
        })
    }

    /// Replaces a message's flags **and** tags (IMAP keywords) with the
    /// server's view, keeping pending local changes like
    /// [`Store::update_flags`]. Returns the message's folder.
    pub fn update_flags_and_tags(
        &self,
        id: &MessageId,
        flags: Flags,
        tags: &[String],
    ) -> Result<FolderId> {
        self.with_tx(|tx| {
            let mut flags = flags;
            let mut tags = tags.to_vec();
            for op in crate::queue::pending_ops_for_message_in_tx(tx, id)? {
                if let Operation::UpdateFlags { change, .. } = op {
                    flags = change.apply_to(flags);
                    change.apply_tags(&mut tags);
                }
            }
            set_flags_in_tx(tx, id, flags)?;
            tags.sort();
            tags.dedup();
            set_tags_in_tx(tx, id, &tags)?;
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
    ///
    /// Messages with queued operations that are still waiting (moved here,
    /// a Sent copy not filed yet, a pending flag change) are kept with
    /// their UID cleared: their UIDs are stale, and the next sync adopts
    /// them again by Message-ID.
    pub fn clear_folder(&self, folder: &FolderId) -> Result<usize> {
        self.with_tx(|tx| {
            folder_account_in_tx(tx, folder)?;
            const WAITING: &str = "SELECT message_id FROM op_queue
                WHERE message_id IS NOT NULL AND state IN ('pending', 'in_flight')";
            tx.execute(
                &format!("UPDATE messages SET uid = NULL WHERE folder_id = ?1 AND id IN ({WAITING})"),
                [folder.as_str()],
            )?;
            let deleted = tx.execute(
                &format!("DELETE FROM messages WHERE folder_id = ?1 AND id NOT IN ({WAITING})"),
                [folder.as_str()],
            )?;
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

    /// Deletes those `candidates` from `blobs` that no message and no draft
    /// attachment references any more (garbage collection after deleting
    /// messages or drafts). Returns how many blob files were removed.
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
            let mut stmt = conn.prepare_cached(REFERENCED_SQL)?;
            let mut removed = 0;
            for hash in candidates {
                if !stmt.exists([hash.as_str()])? && blobs.remove(hash)? {
                    removed += 1;
                }
            }
            Ok(removed)
        })
    }

    /// Whether any message or draft attachment still references this blob
    /// (for garbage collection of the blob store).
    pub fn blob_is_referenced(&self, hash: &BlobHash) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(REFERENCED_SQL, [hash.as_str()], |_| Ok(()))
                .optional()?
                .is_some())
        })
    }
}

/// Whether a blob is referenced by a message or a draft attachment.
const REFERENCED_SQL: &str = "SELECT 1 WHERE EXISTS (SELECT 1 FROM messages WHERE blob_hash = ?1)
    OR EXISTS (SELECT 1 FROM draft_attachments WHERE blob_hash = ?1)";
