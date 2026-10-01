//! The PIM operation queue (`pim_ops`): every write to a CalDAV / CardDAV
//! server goes through here, per source, like the mail queue
//! ([`crate::queue`]) per account.
//!
//! * Item writes are queued by [`Store::save_pim_item`] /
//!   [`Store::delete_pim_item`] / [`Store::move_pim_item`] in the same
//!   transaction as the local change. Collection writes (make / change /
//!   delete a calendar or address book on the server) are queued by the
//!   core with [`Store::enqueue_pim_op`].
//! * Operations name the resource row, not its content: the replayer reads
//!   the current raw text, URL and ETag ([`Store::pim_object_by_id`]) when
//!   it runs, so several edits upload once (a pending upload of the same
//!   item is reused) and a crash never replays stale content.
//! * [`Store::replay_pim_source`] replays one source's operations in order;
//!   a retry ends the pass (later writes never overtake earlier ones).
//!   Operations left `in_flight` by a crash go back to `pending` when the
//!   store opens. A replayed upload records the server's URL and ETag;
//!   once an item has no queued writes left it is `synced`. A confirmed
//!   delete removes the resource row.
//! * A `412 Precondition Failed` is [`PimReplayOutcome::Conflict`]: the
//!   operation fails and the next sync brings the server's version, which
//!   [`Store::apply_server_item`] turns into a conflict copy.

use std::future::Future;
use std::sync::{Arc, PoisonError};

use nail_api::{AccountId, PimSourceId};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::queue::{OpState, ReplayPolicy};
use crate::store::Store;

/// Identifies a queued PIM operation. Increases with enqueue order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PimOpId(pub i64);

impl std::fmt::Display for PimOpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pim-{}", self.0)
    }
}

/// A write to replay on a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PimOperation {
    /// Upload a resource: create it (no URL yet: `If-None-Match: *`) or
    /// replace it (`If-Match` its ETag). Content, URL and ETag are read
    /// from the row when replaying.
    Put {
        /// The `pim_objects` row.
        object_id: i64,
    },
    /// Delete a resource on the server.
    Delete {
        /// The row, kept (marked deleted) until the delete is confirmed;
        /// `None` when the item lives on elsewhere (moved away).
        object_id: Option<i64>,
        /// Its collection at the time.
        collection_id: String,
        /// Its URL at the time; `None` when its upload was still running
        /// (the replayer reads the row's URL then).
        href: Option<String>,
        /// Its ETag at the time.
        etag: Option<String>,
    },
    /// Make a collection on the server (`MKCALENDAR` / extended `MKCOL`).
    CreateCollection {
        /// The collection.
        collection_id: String,
    },
    /// Write a collection's properties (`PROPPATCH`: name, colour, …).
    UpdateCollection {
        /// The collection.
        collection_id: String,
    },
    /// Delete a collection on the server.
    DeleteCollection {
        /// Its URL.
        href: String,
    },
}

impl PimOperation {
    /// Short name stored in `pim_ops.kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Put { .. } => "put",
            Self::Delete { .. } => "delete",
            Self::CreateCollection { .. } => "create-collection",
            Self::UpdateCollection { .. } => "update-collection",
            Self::DeleteCollection { .. } => "delete-collection",
        }
    }

    /// The resource row it concerns.
    pub fn object_id(&self) -> Option<i64> {
        match self {
            Self::Put { object_id } => Some(*object_id),
            Self::Delete { object_id, .. } => *object_id,
            _ => None,
        }
    }
}

/// A row of the PIM queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPimOp {
    /// Queue id; replay order.
    pub id: PimOpId,
    /// The source it replays against.
    pub source_id: PimSourceId,
    /// The operation.
    pub operation: PimOperation,
    /// Current state.
    pub state: OpState,
    /// Replay attempts started so far.
    pub attempts: u32,
    /// Error of the last failed attempt.
    pub last_error: Option<String>,
    /// When it was queued (RFC 3339, UTC).
    pub created_at: String,
    /// When its state last changed (RFC 3339, UTC).
    pub updated_at: String,
}

/// What a [`PimReplayer`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PimReplayOutcome {
    /// The server accepted it; for uploads its URL and new ETag (when the
    /// server said).
    Done {
        /// The resource's URL.
        href: Option<String>,
        /// Its new ETag.
        etag: Option<String>,
    },
    /// A transient problem (offline, timeout): try again later.
    Retry(String),
    /// A permanent problem: do not try again.
    Fail(String),
    /// The server's copy changed (`412`): fails; the next sync makes a
    /// conflict copy.
    Conflict(String),
}

/// Replays PIM operations against servers (the DAV engines in the core).
pub trait PimReplayer: Send + Sync {
    /// Replays one operation; must be idempotent.
    fn replay(&self, op: &QueuedPimOp) -> impl Future<Output = PimReplayOutcome> + Send;
}

/// Summary of a replay pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PimReplayReport {
    /// Replayed.
    pub done: Vec<PimOpId>,
    /// Failed for good (including conflicts).
    pub failed: Vec<PimOpId>,
    /// Asked to be retried (ends the source's pass).
    pub deferred: Vec<PimOpId>,
}

fn state_str(state: OpState) -> &'static str {
    match state {
        OpState::Pending => "pending",
        OpState::InFlight => "in_flight",
        OpState::Done => "done",
        OpState::Failed => "failed",
    }
}

fn parse_state(text: &str) -> OpState {
    match text {
        "in_flight" => OpState::InFlight,
        "done" => OpState::Done,
        "failed" => OpState::Failed,
        _ => OpState::Pending,
    }
}

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";
const OP_COLS: &str = "id, source_id, payload, state, attempts, last_error, created_at, updated_at";

fn op_from_row(row: &Row<'_>) -> rusqlite::Result<QueuedPimOp> {
    let payload: String = row.get(2)?;
    let operation = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(QueuedPimOp {
        id: PimOpId(row.get(0)?),
        source_id: PimSourceId(row.get(1)?),
        operation,
        state: parse_state(&row.get::<_, String>(3)?),
        attempts: row.get(4)?,
        last_error: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// Queues an operation inside `tx`. An upload of a resource that is
/// already queued and not started is reused.
pub(crate) fn enqueue_in_tx(
    tx: &Transaction<'_>,
    source_id: &str,
    operation: &PimOperation,
) -> Result<PimOpId> {
    if let PimOperation::Put { object_id } = operation {
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM pim_ops WHERE source_id = ?1 AND object_id = ?2
                     AND kind = 'put' AND state = 'pending'
                 ORDER BY id LIMIT 1",
                params![source_id, object_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(PimOpId(id));
        }
    }
    tx.execute(
        "INSERT INTO pim_ops (source_id, object_id, kind, payload) VALUES (?1, ?2, ?3, ?4)",
        params![
            source_id,
            operation.object_id(),
            operation.kind(),
            serde_json::to_string(operation)?
        ],
    )?;
    Ok(PimOpId(tx.last_insert_rowid()))
}

/// Fails the not-yet-started operations of a resource (a conflict or a
/// server delete made them moot).
pub(crate) fn cancel_object_ops(tx: &Transaction<'_>, object_id: i64, reason: &str) -> Result<()> {
    tx.execute(
        &format!(
            "UPDATE pim_ops SET state = 'failed', last_error = ?2, updated_at = {NOW}
             WHERE object_id = ?1 AND state = 'pending'"
        ),
        params![object_id, reason],
    )?;
    Ok(())
}

/// The replay lock key of a source (shares the store's per-account lock
/// table; the prefix cannot be an account name).
fn lock_key(source: &PimSourceId) -> AccountId {
    AccountId::new(format!("\u{0}pim:{source}"))
}

struct ClaimGuard<'a> {
    store: &'a Store,
    id: Option<PimOpId>,
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take()
            && let Err(err) = self.store.release_pim_op(id)
        {
            tracing::warn!(%id, %err, "cannot re-queue an interrupted PIM operation");
        }
    }
}

impl Store {
    /// Queues a collection write (or any operation) for a source.
    pub fn enqueue_pim_op(
        &self,
        source: &PimSourceId,
        operation: &PimOperation,
    ) -> Result<PimOpId> {
        self.with_tx(|tx| enqueue_in_tx(tx, source.as_str(), operation))
    }

    /// One queued operation.
    pub fn pim_op(&self, id: PimOpId) -> Result<QueuedPimOp> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT {OP_COLS} FROM pim_ops WHERE id = ?1"),
                [id.0],
                op_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("PIM operation", id))
        })
    }

    /// Queued operations, all or in one state, in queue order.
    pub fn pim_ops(&self, state: Option<OpState>) -> Result<Vec<QueuedPimOp>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {OP_COLS} FROM pim_ops WHERE ?1 IS NULL OR state = ?1 ORDER BY id"
            ))?;
            let rows = stmt.query_map([state.map(state_str)], op_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Sources with operations waiting.
    pub fn sources_with_pending_pim_ops(&self) -> Result<Vec<PimSourceId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT source_id FROM pim_ops WHERE state = 'pending' ORDER BY source_id",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0).map(PimSourceId))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Puts a failed operation back into the queue. Returns whether it was
    /// failed.
    pub fn retry_pim_op(&self, id: PimOpId) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                &format!(
                    "UPDATE pim_ops SET state = 'pending', attempts = 0, updated_at = {NOW}
                     WHERE id = ?1 AND state = 'failed'"
                ),
                [id.0],
            )? > 0)
        })
    }

    /// Deletes `done` operations. Returns how many.
    pub fn purge_done_pim_ops(&self) -> Result<usize> {
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM pim_ops WHERE state = 'done'", [])?))
    }

    /// Crash recovery on open: `in_flight` operations go back to `pending`.
    pub(crate) fn recover_pim_in_flight(&self) -> Result<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                &format!(
                    "UPDATE pim_ops SET state = 'pending', updated_at = {NOW}
                     WHERE state = 'in_flight'"
                ),
                [],
            )?)
        })
    }

    fn release_pim_op(&self, id: PimOpId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                &format!(
                    "UPDATE pim_ops SET state = 'pending', updated_at = {NOW}
                     WHERE id = ?1 AND state = 'in_flight'"
                ),
                [id.0],
            )?;
            Ok(())
        })
    }

    fn claim_next_pim_op(&self, source: &PimSourceId) -> Result<Option<QueuedPimOp>> {
        self.with_tx(|tx| {
            let id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM pim_ops WHERE source_id = ?1 AND state = 'pending'
                     ORDER BY id LIMIT 1",
                    [source.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(id) = id else { return Ok(None) };
            tx.execute(
                &format!(
                    "UPDATE pim_ops SET state = 'in_flight', attempts = attempts + 1,
                     updated_at = {NOW} WHERE id = ?1"
                ),
                [id],
            )?;
            Ok(Some(tx.query_row(
                &format!("SELECT {OP_COLS} FROM pim_ops WHERE id = ?1"),
                [id],
                op_from_row,
            )?))
        })
    }

    /// Records the end of an attempt and what it means for the resource.
    fn finish_pim_op(
        &self,
        op: &QueuedPimOp,
        state: OpState,
        error: Option<&str>,
        done: Option<(Option<String>, Option<String>)>,
    ) -> Result<()> {
        self.with_tx(|tx| {
            tx.execute(
                &format!(
                    "UPDATE pim_ops SET state = ?2, last_error = COALESCE(?3, last_error),
                     updated_at = {NOW} WHERE id = ?1"
                ),
                params![op.id.0, state_str(state), error],
            )?;
            let Some((href, etag)) = done else {
                return Ok(());
            };
            match &op.operation {
                PimOperation::Put { object_id } => {
                    tx.execute(
                        "UPDATE pim_objects SET href = COALESCE(?2, href), etag = ?3
                         WHERE id = ?1",
                        params![object_id, href, etag],
                    )?;
                    tx.execute(
                        "UPDATE pim_objects SET sync = 'synced'
                         WHERE id = ?1 AND sync = 'pending' AND deleted = 0
                           AND NOT EXISTS (SELECT 1 FROM pim_ops WHERE object_id = ?1
                                           AND state IN ('pending', 'in_flight'))",
                        [object_id],
                    )?;
                }
                PimOperation::Delete {
                    object_id: Some(object_id),
                    ..
                } => {
                    tx.execute(
                        "DELETE FROM pim_objects WHERE id = ?1 AND deleted = 1",
                        [object_id],
                    )?;
                }
                _ => {}
            }
            Ok(())
        })
    }

    fn source_lock(&self, source: &PimSourceId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .replay_locks()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        locks.entry(lock_key(source)).or_default().clone()
    }

    /// Replays one source's pending operations in order until none is left
    /// or one asks to be retried. One pass per source at a time. Must run
    /// inside a tokio runtime.
    pub async fn replay_pim_source<R: PimReplayer>(
        &self,
        source: &PimSourceId,
        replayer: &R,
        policy: ReplayPolicy,
    ) -> Result<PimReplayReport> {
        let lock = self.source_lock(source);
        let _pass = lock.lock().await;
        let mut report = PimReplayReport::default();
        loop {
            let claim_source = source.clone();
            let Some(op) = self
                .call(move |s| s.claim_next_pim_op(&claim_source))
                .await?
            else {
                break;
            };
            let mut claim = ClaimGuard {
                store: self,
                id: Some(op.id),
            };
            let outcome = replayer.replay(&op).await;
            let mut deferred = false;
            let (state, error, done) = match outcome {
                PimReplayOutcome::Done { href, etag } => {
                    report.done.push(op.id);
                    (OpState::Done, None, Some((href, etag)))
                }
                PimReplayOutcome::Fail(err) | PimReplayOutcome::Conflict(err) => {
                    tracing::warn!(id = %op.id, error = %err, "PIM operation failed");
                    report.failed.push(op.id);
                    (OpState::Failed, Some(err), None)
                }
                PimReplayOutcome::Retry(err) if op.attempts >= policy.max_attempts => {
                    report.failed.push(op.id);
                    (OpState::Failed, Some(err), None)
                }
                PimReplayOutcome::Retry(err) => {
                    report.deferred.push(op.id);
                    deferred = true;
                    (OpState::Pending, Some(err), None)
                }
            };
            let finished = op.clone();
            self.call(move |s| s.finish_pim_op(&finished, state, error.as_deref(), done))
                .await?;
            claim.id = None;
            if deferred {
                break;
            }
        }
        Ok(report)
    }

    /// Replays every source with pending operations (concurrently, one pass
    /// each). The first error is returned after all passes ended.
    pub async fn replay_pending_pim<R: PimReplayer>(
        &self,
        replayer: &R,
        policy: ReplayPolicy,
    ) -> Result<PimReplayReport> {
        let sources = self.call(Store::sources_with_pending_pim_ops).await?;
        let passes = sources
            .iter()
            .map(|source| self.replay_pim_source(source, replayer, policy));
        let mut report = PimReplayReport::default();
        let mut first_error = None;
        for result in futures_util::future::join_all(passes).await {
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nail_api::pim::{PimItemKind, PimItemSync};

    use super::*;
    use crate::pim::testing::{self, DAV, DAV_CAL, raw};
    use crate::pim::{PimItem, item_ref};

    /// Replays with scripted outcomes and records what it saw.
    struct Script {
        outcomes: Mutex<Vec<PimReplayOutcome>>,
        seen: Mutex<Vec<QueuedPimOp>>,
    }

    impl Script {
        fn new(mut outcomes: Vec<PimReplayOutcome>) -> Self {
            outcomes.reverse();
            Self {
                outcomes: Mutex::new(outcomes),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl PimReplayer for Script {
        fn replay(&self, op: &QueuedPimOp) -> impl Future<Output = PimReplayOutcome> + Send {
            self.seen.lock().unwrap().push(op.clone());
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop()
                .unwrap_or(PimReplayOutcome::Done {
                    href: None,
                    etag: None,
                });
            async move { outcome }
        }
    }

    fn done(href: &str, etag: &str) -> PimReplayOutcome {
        PimReplayOutcome::Done {
            href: Some(href.into()),
            etag: Some(etag.into()),
        }
    }

    fn dav_event(id: &str, uid: &str) -> PimItem {
        let mut e = testing::Fixtures::new().event(id, uid, id);
        e.calendar_id = DAV_CAL.into();
        PimItem::Event(e)
    }

    #[tokio::test]
    async fn uploads_record_url_and_etag() {
        let store = testing::store();
        let item = dav_event("e1", "u1");
        let write = store.save_pim_item(&item, &raw("v1"), 1).unwrap();
        let script = Script::new(vec![done("/cal/u1.ics", "\"1\"")]);
        let report = store
            .replay_pim_source(&PimSourceId::new(DAV), &script, ReplayPolicy::default())
            .await
            .unwrap();
        assert_eq!(report.done, write.op_ids);
        let object = store.pim_object(&item.item_ref()).unwrap().unwrap();
        assert_eq!(object.href.as_deref(), Some("/cal/u1.ics"));
        assert_eq!(object.etag.as_deref(), Some("\"1\""));
        assert_eq!(object.sync, PimItemSync::Synced);
        assert_eq!(store.pim_op(write.op_ids[0]).unwrap().state, OpState::Done);
        assert_eq!(store.purge_done_pim_ops().unwrap(), 1);
        // Delete, replayed: the row goes.
        let deleted = store.delete_pim_item(&item.item_ref(), 2).unwrap();
        store
            .replay_pim_source(&PimSourceId::new(DAV), &script, ReplayPolicy::default())
            .await
            .unwrap();
        assert!(store.pim_object_by_id(deleted.object_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn retries_keep_order_and_failures_stop() {
        let store = testing::store();
        let a = store
            .save_pim_item(&dav_event("a", "ua"), &raw("a"), 1)
            .unwrap();
        let b = store
            .save_pim_item(&dav_event("b", "ub"), &raw("b"), 1)
            .unwrap();
        let source = PimSourceId::new(DAV);
        let script = Script::new(vec![PimReplayOutcome::Retry("offline".into())]);
        let report = store
            .replay_pim_source(&source, &script, ReplayPolicy::default())
            .await
            .unwrap();
        assert_eq!(report.deferred, a.op_ids);
        assert_eq!(script.seen.lock().unwrap().len(), 1);
        assert_eq!(store.sources_with_pending_pim_ops().unwrap().len(), 1);
        assert_eq!(store.sources_with_pending_pim_ops().unwrap()[0], source);
        // Next pass: a conflicts, b goes through.
        let script = Script::new(vec![
            PimReplayOutcome::Conflict("412".into()),
            done("/cal/b.ics", "1"),
        ]);
        let report = store
            .replay_pending_pim(&script, ReplayPolicy::default())
            .await
            .unwrap();
        assert_eq!(report.failed, a.op_ids);
        assert_eq!(report.done, b.op_ids);
        let failed = store.pim_op(a.op_ids[0]).unwrap();
        assert_eq!(failed.state, OpState::Failed);
        assert_eq!(failed.last_error.as_deref(), Some("412"));
        assert_eq!(failed.attempts, 2);
        // a stays pending until the next sync decides.
        assert_eq!(
            store
                .pim_object(&item_ref(PimItemKind::Event, "a"))
                .unwrap()
                .unwrap()
                .sync,
            PimItemSync::Pending
        );
        assert!(store.retry_pim_op(a.op_ids[0]).unwrap());
        assert!(!store.retry_pim_op(a.op_ids[0]).unwrap());
        // Too many retries give up.
        let script = Script::new(vec![PimReplayOutcome::Retry("x".into())]);
        let report = store
            .replay_pim_source(&source, &script, ReplayPolicy { max_attempts: 1 })
            .await
            .unwrap();
        assert_eq!(report.failed, a.op_ids);
    }

    #[tokio::test]
    async fn interrupted_operations_return_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nail.sqlite3");
        let op = {
            let store = Store::open(&path).unwrap();
            store
                .ensure_pim_source(
                    &PimSourceId::new(DAV),
                    nail_api::pim::PimSourceKind::Caldav,
                    0,
                )
                .unwrap();
            let id = store
                .enqueue_pim_op(
                    &PimSourceId::new(DAV),
                    &PimOperation::DeleteCollection {
                        href: "/old/".into(),
                    },
                )
                .unwrap();
            // Claimed, then the process dies.
            assert!(
                store
                    .claim_next_pim_op(&PimSourceId::new(DAV))
                    .unwrap()
                    .is_some()
            );
            assert_eq!(store.pim_op(id).unwrap().state, OpState::InFlight);
            id
        };
        let store = Store::open(&path).unwrap();
        assert_eq!(store.pim_op(op).unwrap().state, OpState::Pending);
        assert_eq!(store.pim_ops(Some(OpState::Pending)).unwrap().len(), 1);
    }

    #[test]
    fn operations_serialize_tagged() {
        let op = PimOperation::Delete {
            object_id: None,
            collection_id: "c".into(),
            href: Some("/x".into()),
            etag: None,
        };
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["type"], "delete");
        assert_eq!(serde_json::from_value::<PimOperation>(json).unwrap(), op);
        assert_eq!(op.kind(), "delete");
        assert_eq!(PimOperation::Put { object_id: 4 }.object_id(), Some(4));
        assert_eq!(PimOpId(3).to_string(), "pim-3");
    }
}
