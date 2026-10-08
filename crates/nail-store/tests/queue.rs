//! Operation queue: local apply, ordering, retry and crash recovery.
#![cfg(test)]

#[expect(dead_code, reason = "these tests use only some of the shared fixtures")]
mod common;

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use common::{account, folder, message, seeded_store};
use nail_api::{FolderId, FolderRole, MessageId};
use nail_store::{
    Error, FlagChange, OpId, OpState, Operation, QueuedOp, ReplayOutcome, ReplayPolicy, Replayer,
    Store,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn mark_seen(id: &str, seen: bool) -> Operation {
    Operation::UpdateFlags {
        message_id: id.into(),
        change: FlagChange {
            seen: Some(seen),
            ..FlagChange::default()
        },
    }
}

fn store_with_messages(ids: &[&str]) -> Store {
    let store = seeded_store();
    for id in ids {
        store
            .upsert_message(&message(id, "a1", "inbox", "2026-01-01T00:00:00Z"))
            .unwrap();
    }
    store
}

/// Records every replayed op and answers from a script (default: Done).
#[derive(Default)]
struct ScriptedReplayer {
    seen: Mutex<Vec<(OpId, u32, MessageId)>>,
    script: Mutex<VecDeque<ReplayOutcome>>,
}

impl ScriptedReplayer {
    fn with_script(outcomes: impl IntoIterator<Item = ReplayOutcome>) -> Self {
        Self {
            script: Mutex::new(outcomes.into_iter().collect()),
            ..Self::default()
        }
    }

    fn replayed_messages(&self) -> Vec<String> {
        let seen = self.seen.lock().unwrap();
        seen.iter().map(|(_, _, m)| m.0.clone()).collect()
    }
}

impl Replayer for ScriptedReplayer {
    fn replay(&self, op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
        let Operation::UpdateFlags { message_id, .. } = &op.operation else {
            unreachable!("only flag updates exist")
        };
        self.seen
            .lock()
            .unwrap()
            .push((op.id, op.attempts, message_id.clone()));
        let outcome = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ReplayOutcome::Done);
        async move {
            tokio::task::yield_now().await;
            outcome
        }
    }
}

#[test]
fn enqueue_applies_locally_and_queues() {
    let store = store_with_messages(&["m1"]);
    let enqueued = store.enqueue(&mark_seen("m1", true)).unwrap();

    assert_eq!(enqueued.changed_folders, [FolderId::from("inbox")]);
    assert_eq!(enqueued.op.state, OpState::Pending);
    assert_eq!(enqueued.op.attempts, 0);
    assert_eq!(enqueued.op.account_id, "a1".into());
    assert_eq!(enqueued.op.operation, mark_seen("m1", true));
    assert!(enqueued.op.created_at.ends_with('Z'));

    // Applied locally at once.
    let m = store.get_message(&"m1".into()).unwrap();
    assert!(m.summary.flags.seen);
    assert_eq!(store.get_folder(&"inbox".into()).unwrap().unread, 0);
    // Other flags untouched.
    assert!(!m.summary.flags.flagged);

    assert_eq!(store.pending_op_count().unwrap(), 1);
    assert_eq!(store.get_op(enqueued.op.id).unwrap(), enqueued.op);
}

#[test]
fn failed_local_apply_queues_nothing() {
    let store = store_with_messages(&["m1"]);
    assert!(
        store
            .enqueue(&mark_seen("missing", true))
            .unwrap_err()
            .is_not_found()
    );
    let empty = Operation::UpdateFlags {
        message_id: "m1".into(),
        change: FlagChange::default(),
    };
    assert!(matches!(store.enqueue(&empty), Err(Error::InvalidData(_))));
    assert_eq!(store.list_ops(None).unwrap().len(), 0);
    assert!(store.get_op(OpId(1)).unwrap_err().is_not_found());
}

#[test]
fn payload_json_is_stable() {
    // The payload is persisted; its shape must not change silently.
    let op = Operation::UpdateFlags {
        message_id: "m1".into(),
        change: FlagChange {
            seen: Some(true),
            flagged: Some(false),
            ..FlagChange::default()
        },
    };
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({"type": "update_flags", "message_id": "m1", "change": {"seen": true, "flagged": false}})
    );
    assert_eq!(op.kind(), "update_flags");
    let back: Operation = serde_json::from_value(json!({
        "type": "update_flags", "message_id": "m1", "change": {"draft": true}
    }))
    .unwrap();
    assert_eq!(
        back,
        Operation::UpdateFlags {
            message_id: "m1".into(),
            change: FlagChange {
                draft: Some(true),
                ..FlagChange::default()
            }
        }
    );
}

#[tokio::test]
async fn replay_runs_in_enqueue_order() {
    let store = store_with_messages(&["m1", "m2", "m3"]);
    let ids: Vec<OpId> = ["m3", "m1", "m2", "m1"]
        .iter()
        .enumerate()
        .map(|(i, m)| store.enqueue(&mark_seen(m, i % 2 == 0)).unwrap().op.id)
        .collect();

    let replayer = ScriptedReplayer::default();
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();

    assert_eq!(report.done, ids);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(report.deferred.is_empty(), "{:?}", report.deferred);
    assert_eq!(replayer.replayed_messages(), ["m3", "m1", "m2", "m1"]);
    assert_eq!(store.pending_op_count().unwrap(), 0);
    let done = store.list_ops(Some(OpState::Done)).unwrap();
    assert_eq!(done.iter().map(|o| o.id).collect::<Vec<_>>(), ids);
    assert!(done.iter().all(|o| o.attempts == 1));

    // Nothing left: a second pass replays nothing.
    let again = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert!(again.done.is_empty(), "{:?}", again.done);

    assert_eq!(store.purge_done_ops().unwrap(), 4);
    assert_eq!(store.list_ops(None).unwrap().len(), 0);
}

/// A store with two queued "mark seen" operations (`m1`, then `m2`).
fn two_queued_ops() -> (Store, OpId, OpId) {
    let store = store_with_messages(&["m1", "m2"]);
    let first = store.enqueue(&mark_seen("m1", true)).unwrap().op.id;
    let second = store.enqueue(&mark_seen("m2", true)).unwrap().op.id;
    (store, first, second)
}

#[tokio::test]
async fn retry_stops_the_pass_and_keeps_order() {
    let (store, first, second) = two_queued_ops();

    let replayer = ScriptedReplayer::with_script([ReplayOutcome::Retry("offline".into())]);
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.deferred, [first]);
    assert!(report.done.is_empty(), "{:?}", report.done);
    assert_eq!(
        replayer.replayed_messages(),
        ["m1"],
        "m2 must not overtake m1"
    );

    let op = store.get_op(first).unwrap();
    assert_eq!(op.state, OpState::Pending);
    assert_eq!(op.attempts, 1);
    assert_eq!(op.last_error.as_deref(), Some("offline"));

    // Next pass: both go through, the first one on its second attempt.
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done, [first, second]);
    let seen = replayer.seen.lock().unwrap().clone();
    assert_eq!(seen[1], (first, 2, "m1".into()));
    assert_eq!(seen[2], (second, 1, "m2".into()));
}

#[tokio::test]
async fn permanent_failure_and_attempt_limit() {
    let store = store_with_messages(&["m1", "m2", "m3"]);
    let a = store.enqueue(&mark_seen("m1", true)).unwrap().op.id;
    let b = store.enqueue(&mark_seen("m2", true)).unwrap().op.id;
    let c = store.enqueue(&mark_seen("m3", true)).unwrap().op.id;

    let replayer = ScriptedReplayer::with_script([
        ReplayOutcome::Fail("no such message on server".into()),
        ReplayOutcome::Retry("timeout".into()),
        ReplayOutcome::Retry("timeout".into()),
    ]);
    let policy = ReplayPolicy { max_attempts: 2 };

    // Pass 1: a fails permanently, the pass continues; b is deferred.
    let report = store.replay_pending(&replayer, policy).await.unwrap();
    assert_eq!(report.failed, [a]);
    assert_eq!(report.deferred, [b]);
    // Pass 2: b reaches the limit and fails; c proceeds.
    let report = store.replay_pending(&replayer, policy).await.unwrap();
    assert_eq!(report.failed, [b]);
    assert_eq!(report.done, [c]);

    let failed = store.list_ops(Some(OpState::Failed)).unwrap();
    assert_eq!(failed.len(), 2);
    assert_eq!(
        failed[0].last_error.as_deref(),
        Some("no such message on server")
    );
    assert_eq!(failed[1].attempts, 2);
    // A failed op stays applied locally (the next sync corrects it).
    assert!(store.get_message(&"m1".into()).unwrap().summary.flags.seen);

    // Manual retry.
    assert!(store.retry_op(a).unwrap());
    assert!(
        !store.retry_op(c).unwrap(),
        "only failed ops can be retried"
    );
    let op = store.get_op(a).unwrap();
    assert_eq!((op.state, op.attempts), (OpState::Pending, 0));
    let report = store.replay_pending(&replayer, policy).await.unwrap();
    assert_eq!(report.done, [a]);
}

/// A replayer that never finishes, to simulate a crash mid-replay.
struct Hanging;

impl Replayer for Hanging {
    fn replay(&self, _op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
        std::future::pending()
    }
}

#[tokio::test]
async fn in_flight_ops_are_recovered_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(nail_store::DB_FILE_NAME);
    let (first, second) = {
        let store = Store::open(&path).unwrap();
        store.upsert_account(&account("a1")).unwrap();
        store
            .upsert_folder(&folder("inbox", "a1", "INBOX", None, FolderRole::Inbox))
            .unwrap();
        for id in ["m1", "m2"] {
            store
                .upsert_message(&message(id, "a1", "inbox", "2026-01-01T00:00:00Z"))
                .unwrap();
        }
        let first = store.enqueue(&mark_seen("m1", true)).unwrap().op.id;
        let second = store.enqueue(&mark_seen("m2", true)).unwrap().op.id;

        // "Crash" while the first op is on the wire: the pass never ends and
        // no destructor runs (unlike a cancellation, see below).
        let mut pass = Box::pin(store.replay_pending(&Hanging, ReplayPolicy::default()));
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut pass)
                .await
                .is_err()
        );
        std::mem::forget(pass);
        assert_eq!(store.get_op(first).unwrap().state, OpState::InFlight);
        (first, second)
    };

    // Restart: the interrupted op is pending again, local changes survived.
    let store = Store::open(&path).unwrap();
    let op = store.get_op(first).unwrap();
    assert_eq!((op.state, op.attempts), (OpState::Pending, 1));
    assert_eq!(store.pending_op_count().unwrap(), 2);
    assert!(store.get_message(&"m1".into()).unwrap().summary.flags.seen);

    let replayer = ScriptedReplayer::default();
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done, [first, second]);
    assert_eq!(store.get_op(first).unwrap().attempts, 2);
}

/// Regression: a cancelled pass (task aborted, shutdown) left the claimed op
/// `in_flight` until the next restart, so it was never replayed.
#[tokio::test]
async fn a_cancelled_pass_puts_the_op_back() {
    let (store, first, second) = two_queued_ops();

    let pass = store.replay_pending(&Hanging, ReplayPolicy::default());
    assert!(
        tokio::time::timeout(Duration::from_millis(200), pass)
            .await
            .is_err()
    );
    let op = store.get_op(first).unwrap();
    assert_eq!((op.state, op.attempts), (OpState::Pending, 1));
    assert_eq!(store.pending_op_count().unwrap(), 2);

    // The next pass (no restart) replays it, in order.
    let replayer = ScriptedReplayer::default();
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done, [first, second]);
}

#[tokio::test]
async fn concurrent_passes_do_not_replay_twice() {
    let store = store_with_messages(&["m1", "m2", "m3", "m4"]);
    for id in ["m1", "m2", "m3", "m4"] {
        store.enqueue(&mark_seen(id, true)).unwrap();
    }
    let replayer = ScriptedReplayer::default();
    let (r1, r2) = tokio::join!(
        store.replay_pending(&replayer, ReplayPolicy::default()),
        store.replay_pending(&replayer, ReplayPolicy::default()),
    );
    assert_eq!(r1.unwrap().done.len() + r2.unwrap().done.len(), 4);
    assert_eq!(replayer.replayed_messages(), ["m1", "m2", "m3", "m4"]);
}

/// Enqueues a follow-up operation while replaying the first one.
struct Chaining {
    store: Store,
    replayed: Mutex<Vec<OpId>>,
}

impl Replayer for Chaining {
    fn replay(&self, op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
        let mut replayed = self.replayed.lock().unwrap();
        if replayed.is_empty() {
            self.store.enqueue(&mark_seen("m2", true)).unwrap();
        }
        replayed.push(op.id);
        std::future::ready(ReplayOutcome::Done)
    }
}

#[tokio::test]
async fn ops_enqueued_during_a_pass_are_replayed_in_it() {
    let store = store_with_messages(&["m1", "m2"]);
    store.enqueue(&mark_seen("m1", true)).unwrap();
    let replayer = Chaining {
        store: store.clone(),
        replayed: Mutex::default(),
    };
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done.len(), 2);
}

#[test]
fn deleting_an_account_drops_its_queue() {
    let store = store_with_messages(&["m1"]);
    store.enqueue(&mark_seen("m1", true)).unwrap();
    store.delete_account(&"a1".into()).unwrap();
    assert_eq!(store.list_ops(None).unwrap().len(), 0);
}
