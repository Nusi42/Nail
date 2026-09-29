//! The operation queue: every user write goes through here.
//!
//! [`Store::enqueue`] applies an [`Operation`] to the local tables **and**
//! records it in `op_queue` in one transaction, so the UI sees the change at
//! once and the change cannot be lost or applied without being queued.
//! A sync engine later calls [`Store::replay_account`] (or
//! [`Store::replay_pending`] for every account) with a [`Replayer`] that
//! performs the operation on the server.
//!
//! # States
//!
//! ```text
//! enqueue ─▶ pending ─claim─▶ in_flight ─Done──────────▶ done
//!               ▲                 │   ─Fail / too many──▶ failed
//!               └──── Retry ──────┘
//!               └─ crash: reset on Store::open
//! ```
//!
//! * Operations of one account replay strictly in enqueue order. A
//!   [`ReplayOutcome::Retry`] ends **that account's** pass, so its later
//!   operations never overtake an earlier one that is still waiting (e.g.
//!   while offline). Other accounts are not affected: each account has its
//!   own pass and its own lock, and [`Store::replay_pending`] runs the
//!   accounts' passes concurrently, so one blocked (or hanging) account
//!   never stalls the others.
//! * An operation that was `in_flight` when the process died goes back to
//!   `pending` when the store is opened again and is replayed once more.
//!   A replayer may therefore see an operation twice and must be idempotent;
//!   operations are expressed as absolute changes ("set \Seen"), not toggles,
//!   to make that easy.
//! * A `failed` operation stays applied locally. The next sync overwrites
//!   the local state with the server's (server wins for flags, design.md
//!   "Sync engine").
//! * While an operation for a message is `pending` or `in_flight`, the sync
//!   merge ([`Store::merge_messages`], [`Store::update_flags`]) re-applies
//!   its flag changes on top of the server's state, and does not touch a
//!   message with a pending move or delete at all.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, PoisonError};

use nail_api::{AccountId, Flags, FolderId, FolderRole, MessageId};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mail::{folder_account_in_tx, message_in_tx, set_flags_in_tx, set_tags_in_tx};
use crate::store::Store;

/// Identifies a queued operation. Increases with enqueue order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OpId(pub i64);

impl std::fmt::Display for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A change to some flags and tags; `None` / empty leaves a value as it is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FlagChange {
    /// New `\Seen` state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen: Option<bool>,
    /// New `\Flagged` state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flagged: Option<bool>,
    /// New `\Answered` state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered: Option<bool>,
    /// New `\Draft` state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    /// Tags (IMAP keywords) to add.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add_tags: Vec<String>,
    /// Tags to remove.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remove_tags: Vec<String>,
}

impl FlagChange {
    /// Returns `flags` with this change applied.
    pub fn apply_to(&self, flags: Flags) -> Flags {
        Flags {
            seen: self.seen.unwrap_or(flags.seen),
            flagged: self.flagged.unwrap_or(flags.flagged),
            answered: self.answered.unwrap_or(flags.answered),
            draft: self.draft.unwrap_or(flags.draft),
        }
    }

    /// Applies the tag part: removes `remove_tags`, then adds `add_tags`
    /// (no duplicates). The result is sorted.
    pub fn apply_tags(&self, tags: &mut Vec<String>) {
        tags.retain(|t| !self.remove_tags.contains(t));
        for tag in &self.add_tags {
            if !tag.is_empty() && !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        tags.sort();
    }

    /// Whether the change sets nothing.
    pub fn is_empty(&self) -> bool {
        self.seen.is_none()
            && self.flagged.is_none()
            && self.answered.is_none()
            && self.draft.is_none()
            && self.add_tags.is_empty()
            && self.remove_tags.is_empty()
    }

    /// Whether the change touches tags.
    pub fn changes_tags(&self) -> bool {
        !self.add_tags.is_empty() || !self.remove_tags.is_empty()
    }
}

impl From<nail_api::FlagChange> for FlagChange {
    fn from(c: nail_api::FlagChange) -> Self {
        Self {
            seen: c.seen,
            flagged: c.flagged,
            answered: c.answered,
            draft: c.draft,
            add_tags: c.add_tags,
            remove_tags: c.remove_tags,
        }
    }
}

/// Where a message was before a move or delete: what the replayer needs
/// to find it on the server. Recorded by [`Store::enqueue`].
///
/// A UID is only meaningful together with the folder's UIDVALIDITY: the
/// replayer compares `uid_validity` with the server's current one and, on
/// a mismatch, finds the message again by `header_message_id` instead of
/// acting on a stale UID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSource {
    /// The account it was in.
    pub account_id: AccountId,
    /// The folder it was in.
    pub folder_id: FolderId,
    /// Its UID in that folder (`None` for a local-only message, or while an
    /// earlier queued operation has not reported it yet).
    pub uid: Option<u32>,
    /// The folder's UIDVALIDITY that `uid` belongs to (`None` when unknown,
    /// e.g. for operations queued before it was recorded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_validity: Option<u32>,
    /// The message's `Message-ID` header (without angle brackets), to find
    /// it again when the UID is unknown or stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_message_id: Option<String>,
}

/// A user-initiated write. Stored as JSON in `op_queue.payload`, so variants
/// and fields may only be added, never renamed or removed, once released.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Operation {
    /// Set or clear system flags and tags of one message.
    UpdateFlags {
        /// The message.
        message_id: MessageId,
        /// The flags to change.
        change: FlagChange,
    },
    /// Move one message to another folder. Within an account the server
    /// moves it (`MOVE`); into another account the replayer copies it to
    /// the target server and deletes it from the source, as one operation.
    /// Locally the message row moves at once (its UID becomes unknown).
    MoveMessage {
        /// The message.
        message_id: MessageId,
        /// The target folder.
        target_folder_id: FolderId,
        /// Filled in by [`Store::enqueue`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<MessageSource>,
    },
    /// Delete one message: move it to its account's Trash, or expunge it
    /// when `permanent` or when it is already in Trash.
    DeleteMessage {
        /// The message.
        message_id: MessageId,
        /// Expunge instead of moving to Trash.
        #[serde(default)]
        permanent: bool,
        /// Filled in by [`Store::enqueue`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<MessageSource>,
        /// The Trash folder it was moved to, filled in by
        /// [`Store::enqueue`]; `None` = expunged (the local row is gone).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trash_folder_id: Option<FolderId>,
    },
    /// File a message that so far exists only locally (a Sent copy) in its
    /// folder on the server (`APPEND`). The local row exists before the
    /// operation is queued and nothing changes locally; the replayer
    /// uploads the row's raw bytes and records the new UID.
    AppendMessage {
        /// The local message (its row holds the raw bytes and flags).
        message_id: MessageId,
        /// The folder to file it in (the row's folder when queued).
        folder_id: FolderId,
    },
}

impl Operation {
    /// The kind name stored in `op_queue.kind` (the serde tag).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UpdateFlags { .. } => "update_flags",
            Self::MoveMessage { .. } => "move_message",
            Self::DeleteMessage { .. } => "delete_message",
            Self::AppendMessage { .. } => "append_message",
        }
    }

    /// The message the operation is about.
    pub fn message_id(&self) -> &MessageId {
        match self {
            Self::UpdateFlags { message_id, .. }
            | Self::MoveMessage { message_id, .. }
            | Self::DeleteMessage { message_id, .. }
            | Self::AppendMessage { message_id, .. } => message_id,
        }
    }

    /// Where the message is on the server before this operation runs (moves
    /// and deletes; filled in by [`Store::enqueue`]).
    pub fn source(&self) -> Option<&MessageSource> {
        match self {
            Self::MoveMessage { source, .. } | Self::DeleteMessage { source, .. } => {
                source.as_ref()
            }
            Self::UpdateFlags { .. } | Self::AppendMessage { .. } => None,
        }
    }

    fn source_mut(&mut self) -> Option<&mut MessageSource> {
        match self {
            Self::MoveMessage { source, .. } | Self::DeleteMessage { source, .. } => {
                source.as_mut()
            }
            Self::UpdateFlags { .. } | Self::AppendMessage { .. } => None,
        }
    }

    /// Whether the operation moves the message out of its folder (the
    /// sync merge must then leave the local row alone).
    pub fn takes_message_away(&self) -> bool {
        matches!(self, Self::MoveMessage { .. } | Self::DeleteMessage { .. })
    }

    /// Applies the operation to the local tables. Returns the account it
    /// belongs to, the folders whose contents changed and the operation
    /// as it is stored (with the source filled in).
    fn apply_local(&self, tx: &Transaction<'_>) -> Result<(AccountId, Vec<FolderId>, Operation)> {
        match self {
            Self::UpdateFlags { message_id, change } => {
                if change.is_empty() {
                    return Err(Error::InvalidData("empty flag change".into()));
                }
                let summary = message_in_tx(tx, message_id)?.summary;
                set_flags_in_tx(tx, message_id, change.apply_to(summary.flags))?;
                if change.changes_tags() {
                    let mut tags = summary.tags.clone();
                    change.apply_tags(&mut tags);
                    set_tags_in_tx(tx, message_id, &tags)?;
                }
                Ok((summary.account_id, vec![summary.folder_id], self.clone()))
            }
            Self::MoveMessage {
                message_id,
                target_folder_id,
                ..
            } => {
                let source = source_in_tx(tx, message_id)?;
                if *target_folder_id == source.folder_id {
                    return Err(Error::InvalidData(format!(
                        "message {message_id} is already in folder {target_folder_id}"
                    )));
                }
                let target_account = folder_account_in_tx(tx, target_folder_id)?;
                move_row(tx, message_id, &target_account, target_folder_id)?;
                let changed = vec![source.folder_id.clone(), target_folder_id.clone()];
                Ok((
                    source.account_id.clone(),
                    changed,
                    Self::MoveMessage {
                        message_id: message_id.clone(),
                        target_folder_id: target_folder_id.clone(),
                        source: Some(source),
                    },
                ))
            }
            Self::DeleteMessage {
                message_id,
                permanent,
                ..
            } => {
                let source = source_in_tx(tx, message_id)?;
                let trash = if *permanent {
                    None
                } else {
                    let trash = role_folder_in_tx(tx, &source.account_id, FolderRole::Trash)?
                        .ok_or_else(|| {
                            Error::not_found("trash folder of account", &source.account_id)
                        })?;
                    (trash != source.folder_id).then_some(trash)
                };
                let mut changed = vec![source.folder_id.clone()];
                match &trash {
                    Some(trash) => {
                        move_row(tx, message_id, &source.account_id, trash)?;
                        changed.push(trash.clone());
                    }
                    None => {
                        tx.execute("DELETE FROM messages WHERE id = ?1", [message_id.as_str()])?;
                    }
                }
                Ok((
                    source.account_id.clone(),
                    changed,
                    Self::DeleteMessage {
                        message_id: message_id.clone(),
                        permanent: *permanent,
                        source: Some(source),
                        trash_folder_id: trash,
                    },
                ))
            }
            Self::AppendMessage {
                message_id,
                folder_id,
            } => {
                let summary = message_in_tx(tx, message_id)?.summary;
                if summary.folder_id != *folder_id {
                    return Err(Error::InvalidData(format!(
                        "message {message_id} is not in folder {folder_id}"
                    )));
                }
                Ok((summary.account_id, vec![summary.folder_id], self.clone()))
            }
        }
    }
}

/// Where a message is now, for [`MessageSource`].
fn source_in_tx(tx: &Transaction<'_>, id: &MessageId) -> Result<MessageSource> {
    let stored = message_in_tx(tx, id)?;
    let uid_validity = match stored.uid {
        Some(_) => tx
            .query_row(
                "SELECT uidvalidity FROM folders WHERE id = ?1",
                [stored.summary.folder_id.as_str()],
                |r| r.get::<_, Option<u32>>(0),
            )
            .optional()?
            .flatten(),
        None => None,
    };
    let header_message_id = tx
        .query_row(
            "SELECT message_id FROM messages WHERE id = ?1",
            [id.as_str()],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    Ok(MessageSource {
        account_id: stored.summary.account_id,
        folder_id: stored.summary.folder_id,
        uid: stored.uid,
        uid_validity,
        header_message_id,
    })
}

/// Moves a message row locally; its UID becomes unknown until the replay
/// (or the next sync) tells.
fn move_row(
    tx: &Transaction<'_>,
    id: &MessageId,
    account: &AccountId,
    folder: &FolderId,
) -> Result<()> {
    tx.execute(
        "UPDATE messages SET account_id = ?2, folder_id = ?3, uid = NULL WHERE id = ?1",
        params![id.as_str(), account.as_str(), folder.as_str()],
    )?;
    Ok(())
}

/// The folder with a role in an account (the first by path).
pub(crate) fn role_folder_in_tx(
    tx: &Transaction<'_>,
    account: &AccountId,
    role: FolderRole,
) -> Result<Option<FolderId>> {
    let role = match serde_json::to_value(role)? {
        serde_json::Value::String(s) => s,
        other => return Err(Error::InvalidData(format!("folder role {other}"))),
    };
    Ok(tx
        .query_row(
            "SELECT id FROM folders WHERE account_id = ?1 AND role = ?2 ORDER BY path LIMIT 1",
            params![account.as_str(), role],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(FolderId))
}

/// Pending and in-flight operations for one message, oldest first.
pub(crate) fn pending_ops_for_message_in_tx(
    tx: &Transaction<'_>,
    id: &MessageId,
) -> Result<Vec<Operation>> {
    let mut stmt = tx.prepare_cached(
        "SELECT payload FROM op_queue
         WHERE message_id = ?1 AND state IN ('pending', 'in_flight') ORDER BY id",
    )?;
    let payloads = stmt
        .query_map([id.as_str()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    payloads
        .iter()
        .map(|p| serde_json::from_str(p).map_err(Error::from))
        .collect()
}
/// Lifecycle state of a queued operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    /// Applied locally, waiting to be replayed.
    Pending,
    /// Currently being replayed.
    InFlight,
    /// Replayed successfully.
    Done,
    /// Given up; see `last_error`.
    Failed,
}

impl OpState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "in_flight",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "in_flight" => Self::InFlight,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

/// A row of the operation queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedOp {
    /// Queue id; replay order.
    pub id: OpId,
    /// Account the operation must be replayed against.
    pub account_id: AccountId,
    /// The operation.
    pub operation: Operation,
    /// Current state.
    pub state: OpState,
    /// Number of replay attempts started so far (including the current one
    /// while `in_flight`).
    pub attempts: u32,
    /// Error of the last failed attempt.
    pub last_error: Option<String>,
    /// When it was enqueued (RFC 3339, UTC).
    pub created_at: String,
    /// When its state last changed (RFC 3339, UTC).
    pub updated_at: String,
}

/// Result of [`Store::enqueue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enqueued {
    /// The queued operation (state `pending`).
    pub op: QueuedOp,
    /// Folders whose contents changed locally, for `Event::MailChanged`.
    pub changed_folders: Vec<FolderId>,
}

/// What a [`Replayer`] reports for one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayOutcome {
    /// The server accepted the operation.
    Done,
    /// A transient problem (offline, timeout); try again in a later pass.
    Retry(String),
    /// A permanent problem (e.g. the message no longer exists on the server);
    /// do not try again.
    Fail(String),
}

/// Performs queued operations against a server. Implemented by the sync
/// engines in `nail-core`.
pub trait Replayer: Send + Sync {
    /// Replays one operation. Must be idempotent: after a crash the same
    /// operation can be replayed again.
    fn replay(&self, op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send;
}

/// Limits for [`Store::replay_pending`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayPolicy {
    /// After this many attempts a [`ReplayOutcome::Retry`] marks the
    /// operation `failed` instead of leaving it `pending`.
    pub max_attempts: u32,
}

impl Default for ReplayPolicy {
    fn default() -> Self {
        Self { max_attempts: 10 }
    }
}

/// Summary of a [`Store::replay_account`] / [`Store::replay_pending`] pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayReport {
    /// Operations that were replayed successfully.
    pub done: Vec<OpId>,
    /// Operations that failed permanently in this pass.
    pub failed: Vec<OpId>,
    /// Operations that asked to be retried; each ended its account's pass
    /// (at most one per account).
    pub deferred: Vec<OpId>,
}

const OP_COLS: &str =
    "id, account_id, payload, state, attempts, last_error, created_at, updated_at";

fn op_from_row(row: &Row<'_>) -> rusqlite::Result<QueuedOp> {
    let payload: String = row.get(2)?;
    let operation = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let state: String = row.get(3)?;
    let state = OpState::parse(&state).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            format!("unknown op state {state:?}").into(),
        )
    })?;
    Ok(QueuedOp {
        id: OpId(row.get(0)?),
        account_id: AccountId(row.get(1)?),
        operation,
        state,
        attempts: row.get(4)?,
        last_error: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn op_in_tx(tx: &Transaction<'_>, id: OpId) -> Result<QueuedOp> {
    tx.query_row(
        &format!("SELECT {OP_COLS} FROM op_queue WHERE id = ?1"),
        [id.0],
        op_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("operation", id))
}

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

impl Store {
    /// Applies `operation` locally and queues it for replay, atomically.
    /// Nothing is changed or queued when the local apply fails (e.g. the
    /// message does not exist). The queued operation (in the result) has
    /// the message's source location filled in for moves and deletes.
    pub fn enqueue(&self, operation: &Operation) -> Result<Enqueued> {
        self.with_tx(|tx| {
            let (account_id, changed_folders, stored) = operation.apply_local(tx)?;
            let payload = serde_json::to_string(&stored)?;
            tx.execute(
                "INSERT INTO op_queue (account_id, kind, payload, message_id) VALUES (?1, ?2, ?3, ?4)",
                params![
                    account_id.as_str(),
                    stored.kind(),
                    payload,
                    stored.message_id().as_str()
                ],
            )?;
            let op = op_in_tx(tx, OpId(tx.last_insert_rowid()))?;
            tracing::debug!(id = %op.id, kind = stored.kind(), "operation enqueued");
            Ok(Enqueued {
                op,
                changed_folders,
            })
        })
    }

    /// Enqueues several operations in one transaction (all or nothing),
    /// e.g. one per selected message.
    pub fn enqueue_all(&self, operations: &[Operation]) -> Result<Vec<Enqueued>> {
        self.with_tx(|tx| {
            let mut out = Vec::with_capacity(operations.len());
            for operation in operations {
                let (account_id, changed_folders, stored) = operation.apply_local(tx)?;
                tx.execute(
                    "INSERT INTO op_queue (account_id, kind, payload, message_id) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        account_id.as_str(),
                        stored.kind(),
                        serde_json::to_string(&stored)?,
                        stored.message_id().as_str()
                    ],
                )?;
                out.push(Enqueued {
                    op: op_in_tx(tx, OpId(tx.last_insert_rowid()))?,
                    changed_folders,
                });
            }
            Ok(out)
        })
    }

    /// The folder with a role (e.g. Trash, Archive) in an account, if any
    /// (the first by path when several have it).
    pub fn folder_with_role(
        &self,
        account: &AccountId,
        role: FolderRole,
    ) -> Result<Option<FolderId>> {
        self.with_tx(|tx| role_folder_in_tx(tx, account, role))
    }

    /// One queued operation.
    pub fn get_op(&self, id: OpId) -> Result<QueuedOp> {
        self.with_tx(|tx| op_in_tx(tx, id))
    }

    /// Queued operations, oldest first; all of them when `state` is `None`.
    pub fn list_ops(&self, state: Option<OpState>) -> Result<Vec<QueuedOp>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {OP_COLS} FROM op_queue WHERE ?1 IS NULL OR state = ?1 ORDER BY id"
            ))?;
            let rows = stmt.query_map([state.map(OpState::as_str)], op_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Operations waiting for replay (`pending` or `in_flight`) that are
    /// about one message, oldest first.
    pub fn pending_ops_for_message(&self, id: &MessageId) -> Result<Vec<QueuedOp>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {OP_COLS} FROM op_queue
                 WHERE message_id = ?1 AND state IN ('pending', 'in_flight') ORDER BY id"
            ))?;
            let rows = stmt.query_map([id.as_str()], op_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Replaces the stored form of a `pending` or `in_flight` operation,
    /// e.g. when a replayer finished part of it and a retry must not repeat
    /// that part. The local tables are not touched. Returns whether the
    /// operation was still waiting.
    pub fn update_op(&self, id: OpId, operation: &Operation) -> Result<bool> {
        let payload = serde_json::to_string(operation)?;
        self.with_conn(|conn| {
            Ok(conn.execute(
                &format!(
                    "UPDATE op_queue SET kind = ?2, payload = ?3, updated_at = {NOW}
                     WHERE id = ?1 AND state IN ('pending', 'in_flight')"
                ),
                params![id.0, operation.kind(), payload],
            )? > 0)
        })
    }

    /// Tells the operations queued after `after` for `message` where an
    /// earlier operation put it on the server: each waiting move or delete
    /// whose recorded source is `folder` with an unknown UID gets `uid` (and
    /// `uid_validity`). Stops at the first operation that takes the message
    /// out of `folder`. Returns how many were updated.
    pub fn rebase_later_ops(
        &self,
        message: &MessageId,
        after: OpId,
        folder: &FolderId,
        uid: u32,
        uid_validity: Option<u32>,
    ) -> Result<usize> {
        self.with_tx(|tx| {
            let rows = {
                let mut stmt = tx.prepare_cached(
                    "SELECT id, payload FROM op_queue
                     WHERE message_id = ?1 AND id > ?2 AND state IN ('pending', 'in_flight')
                     ORDER BY id",
                )?;
                stmt.query_map(params![message.as_str(), after.0], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let mut updated = 0;
            for (id, payload) in rows {
                let mut operation: Operation = serde_json::from_str(&payload)?;
                let takes_away = operation.takes_message_away();
                if let Some(source) = operation.source_mut() {
                    if source.folder_id != *folder {
                        break;
                    }
                    if source.uid.is_none() {
                        source.uid = Some(uid);
                        source.uid_validity = uid_validity;
                        tx.execute(
                            &format!(
                                "UPDATE op_queue SET payload = ?2, updated_at = {NOW} WHERE id = ?1"
                            ),
                            params![id, serde_json::to_string(&operation)?],
                        )?;
                        updated += 1;
                    }
                }
                if takes_away {
                    break;
                }
            }
            Ok(updated)
        })
    }

    /// Number of operations waiting for replay (`pending` or `in_flight`).
    pub fn pending_op_count(&self) -> Result<u32> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM op_queue WHERE state IN ('pending', 'in_flight')",
                [],
                |row| row.get(0),
            )?)
        })
    }

    /// Accounts with operations waiting for replay, in order of their
    /// oldest waiting operation.
    pub fn accounts_with_pending_ops(&self) -> Result<Vec<AccountId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT account_id FROM op_queue WHERE state = 'pending'
                 GROUP BY account_id ORDER BY MIN(id)",
            )?;
            let rows = stmt.query_map([], |r| Ok(AccountId(r.get(0)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Puts a `failed` operation back to `pending` (e.g. a user's "retry").
    /// Its attempt counter restarts. Returns whether it was failed.
    pub fn retry_op(&self, id: OpId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                &format!(
                    "UPDATE op_queue SET state = 'pending', attempts = 0, updated_at = {NOW}
                     WHERE id = ?1 AND state = 'failed'"
                ),
                [id.0],
            )? > 0)
        })
    }

    /// Deletes `done` operations (they are kept for auditing until then).
    /// Returns how many were deleted.
    pub fn purge_done_ops(&self) -> Result<usize> {
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM op_queue WHERE state = 'done'", [])?))
    }

    /// Crash recovery: operations left `in_flight` go back to `pending`.
    /// Runs on every [`Store::open`]. Returns how many were reset.
    pub(crate) fn recover_in_flight(&self) -> Result<usize> {
        let reset = self.with_conn(|conn| {
            Ok(conn.execute(
                &format!(
                    "UPDATE op_queue SET state = 'pending', updated_at = {NOW} WHERE state = 'in_flight'"
                ),
                [],
            )?)
        })?;
        if reset > 0 {
            tracing::info!(count = reset, "re-queued operations interrupted by a crash");
        }
        Ok(reset)
    }

    /// Puts one `in_flight` operation back to `pending`.
    fn release_op(&self, id: OpId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                &format!(
                    "UPDATE op_queue SET state = 'pending', updated_at = {NOW}
                     WHERE id = ?1 AND state = 'in_flight'"
                ),
                [id.0],
            )?;
            Ok(())
        })
    }

    /// Marks the account's oldest `pending` operation `in_flight`, counting
    /// an attempt.
    fn claim_next_op(&self, account: &AccountId) -> Result<Option<QueuedOp>> {
        self.with_tx(|tx| {
            let id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM op_queue WHERE account_id = ?1 AND state = 'pending'
                     ORDER BY id LIMIT 1",
                    [account.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(id) = id else { return Ok(None) };
            tx.execute(
                &format!(
                    "UPDATE op_queue SET state = 'in_flight', attempts = attempts + 1,
                     updated_at = {NOW} WHERE id = ?1"
                ),
                [id],
            )?;
            op_in_tx(tx, OpId(id)).map(Some)
        })
    }

    fn finish_op(&self, id: OpId, state: OpState, error: Option<&str>) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                &format!(
                    "UPDATE op_queue SET state = ?2, last_error = COALESCE(?3, last_error),
                     updated_at = {NOW} WHERE id = ?1"
                ),
                params![id.0, state.as_str(), error],
            )?;
            Ok(())
        })
    }

    /// The replay lock of one account.
    fn account_lock(&self, account: &AccountId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .replay_locks()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        locks.entry(account.clone()).or_default().clone()
    }

    /// Replays one account's pending operations in enqueue order until none
    /// is left or one asks to be retried. Only one pass per account runs at
    /// a time; a concurrent call for the same account waits for the running
    /// one. Passes of different accounts run independently.
    ///
    /// Must be called from within a tokio runtime (database access runs on
    /// the blocking pool). Operations enqueued during the pass are picked up
    /// by the same pass.
    pub async fn replay_account<R: Replayer>(
        &self,
        account: &AccountId,
        replayer: &R,
        policy: ReplayPolicy,
    ) -> Result<ReplayReport> {
        let lock = self.account_lock(account);
        let _pass = lock.lock().await;
        let mut report = ReplayReport::default();
        loop {
            let claim_account = account.clone();
            let Some(op) = self.call(move |s| s.claim_next_op(&claim_account)).await? else {
                break;
            };
            // Puts the op back to `pending` unless it is finished below: the
            // pass may be cancelled while `replay` runs, or `finish_op` may
            // fail. Otherwise it would stay `in_flight` until a restart.
            let mut claim = ClaimGuard {
                store: self,
                id: Some(op.id),
            };
            let outcome = replayer.replay(&op).await;
            let id = op.id;
            let mut deferred = false;
            let (state, error) = match outcome {
                ReplayOutcome::Done => {
                    report.done.push(id);
                    (OpState::Done, None)
                }
                ReplayOutcome::Fail(err) => {
                    tracing::warn!(%id, error = %err, "operation failed permanently");
                    report.failed.push(id);
                    (OpState::Failed, Some(err))
                }
                ReplayOutcome::Retry(err) if op.attempts >= policy.max_attempts => {
                    tracing::warn!(%id, error = %err, attempts = op.attempts, "operation gave up after retries");
                    report.failed.push(id);
                    (OpState::Failed, Some(err))
                }
                ReplayOutcome::Retry(err) => {
                    tracing::debug!(%id, error = %err, "operation deferred");
                    report.deferred.push(id);
                    deferred = true;
                    (OpState::Pending, Some(err))
                }
            };
            self.call(move |s| s.finish_op(id, state, error.as_deref()))
                .await?;
            claim.id = None;
            if deferred {
                break;
            }
        }
        Ok(report)
    }

    /// Replays the pending operations of every account: one
    /// [`Store::replay_account`] pass per account, run concurrently, so an
    /// account that is offline (retry) or slow does not hold up the others.
    /// The report lists all accounts' results (per-account order is kept;
    /// `deferred` has at most one operation per account). When a pass
    /// fails, the others still finish and the first error is returned.
    pub async fn replay_pending<R: Replayer>(
        &self,
        replayer: &R,
        policy: ReplayPolicy,
    ) -> Result<ReplayReport> {
        let accounts = self.call(Store::accounts_with_pending_ops).await?;
        let passes = accounts
            .iter()
            .map(|account| self.replay_account(account, replayer, policy));
        let results = futures_util::future::join_all(passes).await;
        let mut report = ReplayReport::default();
        let mut first_error = None;
        for result in results {
            match result {
                Ok(r) => {
                    report.done.extend(r.done);
                    report.failed.extend(r.failed);
                    report.deferred.extend(r.deferred);
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        match first_error {
            Some(err) => Err(err),
            None => Ok(report),
        }
    }
}

/// An operation claimed by a replay pass; see [`Store::replay_account`].
struct ClaimGuard<'a> {
    store: &'a Store,
    /// `None` once the operation is finished.
    id: Option<OpId>,
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            // A short, local write; fine to do synchronously here.
            match self.store.release_op(id) {
                Ok(()) => tracing::debug!(%id, "interrupted operation re-queued"),
                Err(err) => tracing::warn!(%id, %err, "cannot re-queue an interrupted operation"),
            }
        }
    }
}

/// Replay locks by account (see [`Store::replay_account`]).
pub(crate) type ReplayLocks = std::sync::Mutex<HashMap<AccountId, Arc<tokio::sync::Mutex<()>>>>;
