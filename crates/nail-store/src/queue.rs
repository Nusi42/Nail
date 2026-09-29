//! The operation queue: every user write goes through here.
//!
//! [`Store::enqueue`] applies an [`Operation`] to the local tables **and**
//! records it in `op_queue` in one transaction, so the UI sees the change at
//! once and the change cannot be lost or applied without being queued.
//! A sync engine later calls [`Store::replay_pending`] with a [`Replayer`]
//! that performs the operation on the server.
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
//! * Operations replay strictly in enqueue order. A [`ReplayOutcome::Retry`]
//!   ends the pass, so later operations never overtake an earlier one that
//!   is still waiting (e.g. while offline).
//! * An operation that was `in_flight` when the process died goes back to
//!   `pending` when the store is opened again and is replayed once more.
//!   A replayer may therefore see an operation twice and must be idempotent;
//!   operations are expressed as absolute changes ("set \Seen"), not toggles,
//!   to make that easy.
//! * A `failed` operation stays applied locally. The next sync overwrites
//!   the local state with the server's (server wins for flags, design.md
//!   "Sync engine").

use std::future::Future;

use nail_api::{AccountId, Flags, FolderId, MessageId};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mail::{message_in_tx, set_flags_in_tx};
use crate::store::Store;

/// Identifies a queued operation. Increases with enqueue order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OpId(pub i64);

impl std::fmt::Display for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A change to some flags; `None` leaves a flag as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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

    /// Whether the change sets nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A user-initiated write. Stored as JSON in `op_queue.payload`, so variants
/// and fields may only be added, never renamed or removed, once released.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Operation {
    /// Set or clear system flags of one message.
    UpdateFlags {
        /// The message.
        message_id: MessageId,
        /// The flags to change.
        change: FlagChange,
    },
}

impl Operation {
    /// The kind name stored in `op_queue.kind` (the serde tag).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UpdateFlags { .. } => "update_flags",
        }
    }

    /// Applies the operation to the local tables. Returns the account it
    /// belongs to and the folders whose contents changed.
    fn apply_local(&self, tx: &Transaction<'_>) -> Result<(AccountId, Vec<FolderId>)> {
        match self {
            Self::UpdateFlags { message_id, change } => {
                if change.is_empty() {
                    return Err(Error::InvalidData("empty flag change".into()));
                }
                let summary = message_in_tx(tx, message_id)?.summary;
                set_flags_in_tx(tx, message_id, change.apply_to(summary.flags))?;
                Ok((summary.account_id, vec![summary.folder_id]))
            }
        }
    }
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

/// Summary of one [`Store::replay_pending`] pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayReport {
    /// Operations that were replayed successfully.
    pub done: Vec<OpId>,
    /// Operations that failed permanently in this pass.
    pub failed: Vec<OpId>,
    /// The operation that asked to be retried and ended the pass, if any.
    pub deferred: Option<OpId>,
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
    /// message does not exist).
    pub fn enqueue(&self, operation: &Operation) -> Result<Enqueued> {
        let payload = serde_json::to_string(operation)?;
        self.with_tx(|tx| {
            let (account_id, changed_folders) = operation.apply_local(tx)?;
            tx.execute(
                "INSERT INTO op_queue (account_id, kind, payload) VALUES (?1, ?2, ?3)",
                params![account_id.as_str(), operation.kind(), payload],
            )?;
            let op = op_in_tx(tx, OpId(tx.last_insert_rowid()))?;
            tracing::debug!(id = %op.id, kind = operation.kind(), "operation enqueued");
            Ok(Enqueued {
                op,
                changed_folders,
            })
        })
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

    /// Marks the oldest `pending` operation `in_flight`, counting an attempt.
    fn claim_next_op(&self) -> Result<Option<QueuedOp>> {
        self.with_tx(|tx| {
            let id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM op_queue WHERE state = 'pending' ORDER BY id LIMIT 1",
                    [],
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

    /// Replays pending operations in enqueue order until the queue is empty
    /// or an operation asks to be retried. Only one pass runs at a time per
    /// store; a concurrent call waits for the running one.
    ///
    /// Must be called from within a tokio runtime (database access runs on
    /// the blocking pool). Operations enqueued during the pass are picked up
    /// by the same pass.
    pub async fn replay_pending<R: Replayer>(
        &self,
        replayer: &R,
        policy: ReplayPolicy,
    ) -> Result<ReplayReport> {
        let _pass = self.replay_lock().lock().await;
        let mut report = ReplayReport::default();
        while let Some(op) = self.call(Store::claim_next_op).await? {
            // Puts the op back to `pending` unless it is finished below: the
            // pass may be cancelled while `replay` runs, or `finish_op` may
            // fail. Otherwise it would stay `in_flight` until a restart.
            let mut claim = ClaimGuard {
                store: self,
                id: Some(op.id),
            };
            let outcome = replayer.replay(&op).await;
            let id = op.id;
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
                    report.deferred = Some(id);
                    (OpState::Pending, Some(err))
                }
            };
            self.call(move |s| s.finish_op(id, state, error.as_deref()))
                .await?;
            claim.id = None;
            if report.deferred.is_some() {
                break;
            }
        }
        Ok(report)
    }
}

/// An operation claimed by a replay pass; see [`Store::replay_pending`].
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
