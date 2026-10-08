//! Grouped replay (P3, ADR 0164): consecutive operations of one account
//! with the same group key reach a replayer together, so an engine can
//! send one server call (an IMAP UID set, a Gmail `batchModify`, an EWS
//! multi-item request).
#![cfg(test)]

#[expect(dead_code, reason = "these tests use only some of the shared fixtures")]
mod common;

use std::future::Future;
use std::sync::Mutex;

use common::{folder, message, seeded_store};
use nail_api::{FolderId, FolderRole};
use nail_store::{
    FlagChange, OpState, Operation, QueuedOp, ReplayOutcome, ReplayPolicy, Replayer, Store,
};
use pretty_assertions::assert_eq;

fn seen(id: &str) -> Operation {
    Operation::UpdateFlags {
        message_id: id.into(),
        change: FlagChange {
            seen: Some(true),
            ..FlagChange::default()
        },
    }
}

fn archive(id: &str) -> Operation {
    Operation::MoveMessage {
        message_id: id.into(),
        target_folder_id: FolderId::from("archive"),
        source: None,
    }
}

/// a1 with inbox, archive and trash and five messages in the Inbox.
fn store() -> Store {
    let store = seeded_store();
    store
        .upsert_folder(&folder("trash", "a1", "Trash", None, FolderRole::Trash))
        .unwrap();
    for n in 1..=5 {
        let mut m = message(&format!("m{n}"), "a1", "inbox", "2026-01-01T00:00:00Z");
        m.uid = Some(n);
        store.upsert_message(&m).unwrap();
    }
    store
}

/// Records the groups it sees; answers with `answer` for each group.
struct Grouping {
    max: usize,
    groups: Mutex<Vec<Vec<String>>>,
    answer: fn(usize) -> Vec<ReplayOutcome>,
}

impl Grouping {
    fn new(max: usize, answer: fn(usize) -> Vec<ReplayOutcome>) -> Self {
        Self {
            max,
            groups: Mutex::new(Vec::new()),
            answer,
        }
    }

    fn groups(&self) -> Vec<Vec<String>> {
        self.groups.lock().unwrap().clone()
    }
}

impl Replayer for Grouping {
    fn replay(&self, op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
        self.groups
            .lock()
            .unwrap()
            .push(vec![op.operation.message_id().to_string()]);
        let mut outcome = (self.answer)(1);
        std::future::ready(outcome.remove(0))
    }

    fn max_group(&self) -> usize {
        self.max
    }

    fn replay_group(&self, ops: &[QueuedOp]) -> impl Future<Output = Vec<ReplayOutcome>> + Send {
        self.groups.lock().unwrap().push(
            ops.iter()
                .map(|op| op.operation.message_id().to_string())
                .collect(),
        );
        std::future::ready((self.answer)(ops.len()))
    }
}

fn all_done(n: usize) -> Vec<ReplayOutcome> {
    vec![ReplayOutcome::Done; n]
}

#[tokio::test]
async fn consecutive_operations_with_one_key_replay_together() {
    let store = store();
    let ops = [
        seen("m1"),
        seen("m2"),
        seen("m3"),
        archive("m1"),
        archive("m2"),
        archive("m4"),
        seen("m5"),
    ];
    store.enqueue_all(&ops).unwrap();
    let replayer = Grouping::new(10, all_done);
    let report = store
        .replay_account(&"a1".into(), &replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done.len(), 7);
    assert_eq!(
        replayer.groups(),
        [
            vec!["m1", "m2", "m3"],
            vec!["m1", "m2", "m4"],
            // Not joined with the first flags: other operations came between.
            vec!["m5"],
        ]
    );
    assert_eq!(store.list_ops(Some(OpState::Pending)).unwrap().len(), 0);
}

#[tokio::test]
async fn groups_respect_the_replayers_limit_and_a_message_once() {
    let store = store();
    store
        .enqueue_all(&[seen("m1"), seen("m2"), seen("m3"), seen("m4"), seen("m5")])
        .unwrap();
    let replayer = Grouping::new(2, all_done);
    store
        .replay_account(&"a1".into(), &replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(
        replayer.groups(),
        [vec!["m1", "m2"], vec!["m3", "m4"], vec!["m5"]]
    );
    // The same message twice ends a group.
    let store = self::store();
    store.enqueue_all(&[seen("m1"), seen("m1")]).unwrap();
    let replayer = Grouping::new(10, all_done);
    store
        .replay_account(&"a1".into(), &replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(replayer.groups(), [vec!["m1"], vec!["m1"]]);
}

#[tokio::test]
async fn a_default_replayer_replays_one_by_one() {
    struct One(Mutex<usize>);
    impl Replayer for One {
        fn replay(&self, _op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
            *self.0.lock().unwrap() += 1;
            std::future::ready(ReplayOutcome::Done)
        }
    }
    let store = store();
    store.enqueue_all(&[seen("m1"), seen("m2")]).unwrap();
    let one = One(Mutex::new(0));
    let report = store
        .replay_account(&"a1".into(), &one, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!((report.done.len(), *one.0.lock().unwrap()), (2, 2));
}

#[tokio::test]
async fn a_short_answer_leaves_the_rest_pending_without_an_attempt() {
    let store = store();
    let queued = store
        .enqueue_all(&[seen("m1"), seen("m2"), seen("m3")])
        .unwrap();
    // The first is done, the second asks to wait, the third was not tried.
    let replayer = Grouping::new(10, |_| {
        vec![ReplayOutcome::Done, ReplayOutcome::Retry("offline".into())]
    });
    let report = store
        .replay_account(&"a1".into(), &replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done, [queued[0].op.id]);
    assert_eq!(report.deferred, [queued[1].op.id]);
    let second = store.get_op(queued[1].op.id).unwrap();
    let third = store.get_op(queued[2].op.id).unwrap();
    assert_eq!((second.state, second.attempts), (OpState::Pending, 1));
    assert_eq!((third.state, third.attempts), (OpState::Pending, 0));
}

#[tokio::test]
async fn failures_stay_per_operation() {
    let store = store();
    let queued = store.enqueue_all(&[archive("m1"), archive("m2")]).unwrap();
    let replayer = Grouping::new(10, |_| {
        vec![ReplayOutcome::Fail("gone".into()), ReplayOutcome::Done]
    });
    let report = store
        .replay_account(&"a1".into(), &replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.failed, [queued[0].op.id]);
    assert_eq!(report.done, [queued[1].op.id]);
}

#[test]
fn group_keys() {
    let store = store();
    let queued = store
        .enqueue_all(&[
            seen("m1"),
            archive("m2"),
            Operation::DeleteMessage {
                message_id: "m3".into(),
                permanent: false,
                source: None,
                trash_folder_id: None,
            },
        ])
        .unwrap();
    let key = |i: usize| queued[i].op.operation.group_key().unwrap();
    assert!(key(0).starts_with("flags:"));
    assert_eq!(key(1), "move:a1:inbox:archive");
    assert_eq!(key(2), "delete:a1:inbox:false:trash");
    assert_eq!(
        Operation::AppendMessage {
            message_id: "m1".into(),
            folder_id: "inbox".into(),
        }
        .group_key(),
        None
    );
    // Moves without their recorded source never group.
    assert_eq!(archive("m1").group_key(), None);
}
