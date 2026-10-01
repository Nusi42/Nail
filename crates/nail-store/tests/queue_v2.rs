//! Operation queue v2: per-account passes, moves, deletes, tags and the
//! sync-side merge that keeps pending local changes.

mod common;

use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use common::{account, folder, message, seeded_store};
use nail_api::{AccountId, Flags, FolderId, FolderRole, MessageId};
use nail_store::{
    Error, FlagChange, MessageMeta, MessageSource, OpState, Operation, QueuedOp, ReplayOutcome,
    ReplayPolicy, Replayer, Store, SyncedMessage,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn seen(id: &str, value: bool) -> Operation {
    Operation::UpdateFlags {
        message_id: id.into(),
        change: FlagChange {
            seen: Some(value),
            ..FlagChange::default()
        },
    }
}

/// Two accounts: a1 (inbox, archive, trash) and a2 (inbox).
fn two_account_store() -> Store {
    let store = seeded_store();
    store
        .upsert_folder(&folder("trash", "a1", "Trash", None, FolderRole::Trash))
        .unwrap();
    store.upsert_account(&account("a2")).unwrap();
    store
        .upsert_folder(&folder("inbox2", "a2", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    for (id, acc, f) in [
        ("m1", "a1", "inbox"),
        ("m2", "a1", "inbox"),
        ("n1", "a2", "inbox2"),
        ("n2", "a2", "inbox2"),
    ] {
        let mut m = message(id, acc, f, "2026-01-01T00:00:00Z");
        m.uid = Some(u32::from(id.as_bytes()[1] - b'0'));
        store.upsert_message(&m).unwrap();
    }
    store
}

/// Answers per account: a1 is offline (retry), everything else succeeds.
#[derive(Default)]
struct OfflineAccount {
    offline: Option<AccountId>,
    replayed: Mutex<Vec<(AccountId, MessageId)>>,
}

impl Replayer for OfflineAccount {
    fn replay(&self, op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
        self.replayed
            .lock()
            .unwrap()
            .push((op.account_id.clone(), op.operation.message_id().clone()));
        let outcome = if self.offline.as_ref() == Some(&op.account_id) {
            ReplayOutcome::Retry("offline".into())
        } else {
            ReplayOutcome::Done
        };
        std::future::ready(outcome)
    }
}

#[tokio::test]
async fn a_blocked_account_does_not_stall_others() {
    let store = two_account_store();
    let a1_first = store.enqueue(&seen("m1", true)).unwrap().op.id;
    let n1 = store.enqueue(&seen("n1", true)).unwrap().op.id;
    let _a1_second = store.enqueue(&seen("m2", true)).unwrap().op.id;
    let n2 = store.enqueue(&seen("n2", true)).unwrap().op.id;
    assert_eq!(
        store.accounts_with_pending_ops().unwrap(),
        [AccountId::from("a1"), AccountId::from("a2")]
    );

    let replayer = OfflineAccount {
        offline: Some("a1".into()),
        ..OfflineAccount::default()
    };
    let report = store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.deferred, [a1_first]);
    assert_eq!(report.done, [n1, n2], "a2 replays although a1 is offline");
    // a1's second op was not tried (order within the account holds).
    let replayed = replayer.replayed.lock().unwrap().clone();
    assert!(!replayed.contains(&("a1".into(), "m2".into())));
    assert_eq!(store.pending_op_count().unwrap(), 2);
    assert_eq!(
        store.accounts_with_pending_ops().unwrap(),
        [AccountId::from("a1")]
    );
}

/// Never finishes for a1, succeeds at once for others.
struct HangingAccount;

impl Replayer for HangingAccount {
    fn replay(&self, op: &QueuedOp) -> impl Future<Output = ReplayOutcome> + Send {
        let hang = op.account_id.as_str() == "a1";
        async move {
            if hang {
                std::future::pending::<()>().await;
            }
            ReplayOutcome::Done
        }
    }
}

#[tokio::test]
async fn a_hanging_account_does_not_stall_others() {
    let store = two_account_store();
    let _stuck = store.enqueue(&seen("m1", true)).unwrap().op.id;
    let n1 = store.enqueue(&seen("n1", true)).unwrap().op.id;

    // A pass for everything never finishes (a1 hangs) …
    let all = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .replay_pending(&HangingAccount, ReplayPolicy::default())
                .await
        }
    });
    // … but a2's operation is done meanwhile.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while store.get_op(n1).unwrap().state != OpState::Done {
        assert!(tokio::time::Instant::now() < deadline, "a2 stalled");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    all.abort();
}

#[tokio::test]
async fn account_passes_are_independent_and_serialized_per_account() {
    let store = two_account_store();
    let m1 = store.enqueue(&seen("m1", true)).unwrap().op.id;
    let n1 = store.enqueue(&seen("n1", true)).unwrap().op.id;
    let replayer = OfflineAccount::default();
    let report = store
        .replay_account(&"a2".into(), &replayer, ReplayPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.done, [n1]);
    assert_eq!(store.get_op(m1).unwrap().state, OpState::Pending);

    // Two concurrent passes of the same account replay each op once.
    store.enqueue(&seen("m2", true)).unwrap();
    let a1 = AccountId::from("a1");
    let (r1, r2) = tokio::join!(
        store.replay_account(&a1, &replayer, ReplayPolicy::default()),
        store.replay_account(&a1, &replayer, ReplayPolicy::default()),
    );
    assert_eq!(r1.unwrap().done.len() + r2.unwrap().done.len(), 2);
    let replayed = replayer.replayed.lock().unwrap().clone();
    assert_eq!(replayed.len(), 3);
}

#[test]
fn move_applies_locally_and_records_the_source() {
    let store = two_account_store();
    let op = Operation::MoveMessage {
        message_id: "m1".into(),
        target_folder_id: "archive".into(),
        source: None,
    };
    let enqueued = store.enqueue(&op).unwrap();
    assert_eq!(
        enqueued.changed_folders,
        [FolderId::from("inbox"), FolderId::from("archive")]
    );
    let Operation::MoveMessage { source, .. } = &enqueued.op.operation else {
        panic!("not a move: {:?}", enqueued.op.operation);
    };
    assert_eq!(
        source.as_ref(),
        Some(&MessageSource {
            account_id: "a1".into(),
            folder_id: "inbox".into(),
            uid: Some(1),
            uid_validity: None,
            header_message_id: None,
            remote_id: None,
        })
    );
    let moved = store.get_message(&"m1".into()).unwrap();
    assert_eq!(moved.summary.folder_id, "archive".into());
    assert_eq!(moved.uid, None, "the new UID is unknown until the replay");

    // The payload is persisted with the source.
    let payload = serde_json::to_value(&enqueued.op.operation).unwrap();
    assert_eq!(
        payload,
        json!({"type": "move_message", "message_id": "m1", "target_folder_id": "archive",
               "source": {"account_id": "a1", "folder_id": "inbox", "uid": 1}})
    );

    // Moving into the folder it is in is refused.
    let same = Operation::MoveMessage {
        message_id: "m1".into(),
        target_folder_id: "archive".into(),
        source: None,
    };
    assert!(matches!(store.enqueue(&same), Err(Error::InvalidData(_))));

    // After the replay the sync engine records the new UID.
    store.set_message_uid(&"m1".into(), Some(42)).unwrap();
    assert_eq!(
        store
            .find_message_by_uid(&"archive".into(), 42)
            .unwrap()
            .unwrap()
            .summary
            .id,
        "m1".into()
    );
}

#[test]
fn cross_account_move_belongs_to_the_source_account() {
    let store = two_account_store();
    let enqueued = store
        .enqueue(&Operation::MoveMessage {
            message_id: "m1".into(),
            target_folder_id: "inbox2".into(),
            source: None,
        })
        .unwrap();
    assert_eq!(enqueued.op.account_id, "a1".into());
    let moved = store.get_message(&"m1".into()).unwrap().summary;
    assert_eq!(
        (moved.account_id, moved.folder_id),
        ("a2".into(), "inbox2".into())
    );
}

#[test]
fn delete_moves_to_trash_or_expunges() {
    let store = two_account_store();
    let to_trash = store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m1".into(),
            permanent: false,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    let Operation::DeleteMessage {
        trash_folder_id, ..
    } = &to_trash.op.operation
    else {
        panic!()
    };
    assert_eq!(trash_folder_id.as_ref(), Some(&FolderId::from("trash")));
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.folder_id,
        "trash".into()
    );

    // Deleting again from Trash expunges.
    let expunge = store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m1".into(),
            permanent: false,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    let Operation::DeleteMessage {
        trash_folder_id,
        source,
        ..
    } = &expunge.op.operation
    else {
        panic!()
    };
    assert_eq!(*trash_folder_id, None);
    assert_eq!(source.as_ref().unwrap().folder_id, "trash".into());
    assert!(store.get_message(&"m1".into()).unwrap_err().is_not_found());

    // Permanent delete.
    store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m2".into(),
            permanent: true,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    assert!(store.get_message(&"m2".into()).unwrap_err().is_not_found());

    // No Trash in a2: a non-permanent delete is refused and nothing queued.
    let before = store.list_ops(None).unwrap().len();
    let err = store
        .enqueue(&Operation::DeleteMessage {
            message_id: "n1".into(),
            permanent: false,
            source: None,
            trash_folder_id: None,
        })
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
    assert_eq!(store.list_ops(None).unwrap().len(), before);
    assert_eq!(
        store
            .folder_with_role(&"a1".into(), FolderRole::Trash)
            .unwrap(),
        Some("trash".into())
    );
}

#[test]
fn tag_changes_apply_locally() {
    let store = two_account_store();
    store
        .enqueue(&Operation::UpdateFlags {
            message_id: "m1".into(),
            change: FlagChange {
                add_tags: vec!["work".into(), "$label1".into()],
                ..FlagChange::default()
            },
        })
        .unwrap();
    store
        .enqueue(&Operation::UpdateFlags {
            message_id: "m1".into(),
            change: FlagChange {
                remove_tags: vec!["$label1".into()],
                add_tags: vec!["later".into()],
                ..FlagChange::default()
            },
        })
        .unwrap();
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.tags,
        ["later", "work"]
    );
    let api = nail_api::FlagChange {
        flagged: Some(true),
        add_tags: vec!["x".into()],
        ..nail_api::FlagChange::default()
    };
    let change = FlagChange::from(api);
    assert_eq!(change.flagged, Some(true));
    assert_eq!(change.add_tags, ["x"]);
    // Old payloads without tags still parse, new ones omit empty lists.
    assert_eq!(
        serde_json::to_value(&change).unwrap(),
        json!({"flagged": true, "add_tags": ["x"]})
    );
}

#[test]
fn enqueue_all_is_atomic() {
    let store = two_account_store();
    let ops = [seen("m1", true), seen("missing", true)];
    assert!(store.enqueue_all(&ops).unwrap_err().is_not_found());
    assert!(store.list_ops(None).unwrap().is_empty());
    assert!(!store.get_message(&"m1".into()).unwrap().summary.flags.seen);
    let done = store
        .enqueue_all(&[seen("m1", true), seen("n1", true)])
        .unwrap();
    assert_eq!(done.len(), 2);
    assert_eq!(done[1].op.account_id, "a2".into());
}

fn server_copy(id: &str, account: &str, folder: &str, uid: u32, flags: Flags) -> SyncedMessage {
    let mut m = message(id, account, folder, "2026-01-01T00:00:00Z");
    m.uid = Some(uid);
    m.summary.flags = flags;
    SyncedMessage {
        message: m,
        meta: None,
    }
}

#[tokio::test]
async fn sync_merge_keeps_pending_flags_until_replayed() {
    let store = two_account_store();
    store
        .enqueue(&Operation::UpdateFlags {
            message_id: "m1".into(),
            change: FlagChange {
                seen: Some(true),
                add_tags: vec!["todo".into()],
                ..FlagChange::default()
            },
        })
        .unwrap();

    // The server still says unread but newly flagged (a change made
    // elsewhere): the pending \Seen wins, the server's \Flagged arrives.
    let server = Flags {
        flagged: true,
        ..Flags::default()
    };
    let report = store
        .merge_messages(&[
            server_copy("m1", "a1", "inbox", 1, server),
            server_copy("m2", "a1", "inbox", 2, server),
        ])
        .unwrap();
    assert_eq!(report.written, 2);
    assert_eq!(report.kept_local, [MessageId::from("m1")]);
    let m1 = store.get_message(&"m1".into()).unwrap().summary;
    assert!(m1.flags.seen && m1.flags.flagged);
    assert_eq!(m1.tags, ["todo"]);
    let m2 = store.get_message(&"m2".into()).unwrap().summary;
    assert!(!m2.flags.seen && m2.flags.flagged);

    // The flag-only sync path behaves the same.
    store.update_flags(&"m1".into(), Flags::default()).unwrap();
    assert!(store.get_message(&"m1".into()).unwrap().summary.flags.seen);
    store
        .update_flags_and_tags(&"m1".into(), Flags::default(), &["server".into()])
        .unwrap();
    let m1 = store.get_message(&"m1".into()).unwrap().summary;
    assert!(m1.flags.seen);
    assert_eq!(m1.tags, ["server", "todo"]);
    assert_eq!(
        store.pending_ops_for_message(&"m1".into()).unwrap().len(),
        1
    );

    // Once replayed, the server wins again.
    let replayer = OfflineAccount::default();
    store
        .replay_pending(&replayer, ReplayPolicy::default())
        .await
        .unwrap();
    store
        .merge_messages(&[server_copy("m1", "a1", "inbox", 1, Flags::default())])
        .unwrap();
    let m1 = store.get_message(&"m1".into()).unwrap().summary;
    assert!(!m1.flags.seen);
    assert!(m1.tags.is_empty());
}

#[test]
fn sync_merge_skips_messages_with_pending_moves_or_deletes() {
    let store = two_account_store();
    store
        .enqueue(&Operation::MoveMessage {
            message_id: "m1".into(),
            target_folder_id: "archive".into(),
            source: None,
        })
        .unwrap();
    store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m2".into(),
            permanent: true,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    // The server still has both in INBOX until the replay.
    let report = store
        .merge_messages(&[
            server_copy("m1", "a1", "inbox", 1, Flags::default()),
            server_copy("m2", "a1", "inbox", 2, Flags::default()),
        ])
        .unwrap();
    assert_eq!(report.written, 0);
    assert_eq!(
        report.skipped,
        [MessageId::from("m1"), MessageId::from("m2")]
    );
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.folder_id,
        "archive".into()
    );
    assert!(store.get_message(&"m2".into()).unwrap_err().is_not_found());
    // The legacy upsert path merges the same way.
    store
        .upsert_messages(&[server_copy("m1", "a1", "inbox", 1, Flags::default()).message])
        .unwrap();
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.folder_id,
        "archive".into()
    );
}

#[test]
fn merge_records_headers_and_size() {
    let store = two_account_store();
    let mut synced = server_copy("m1", "a1", "inbox", 1, Flags::default());
    synced.meta = Some(MessageMeta {
        message_id: Some("<m1@example.org>".into()),
        in_reply_to: vec![" <root@example.org> ".into()],
        references: vec!["<root@example.org>".into(), "".into()],
        size: Some(2048),
    });
    store.merge_messages(&[synced]).unwrap();
    assert_eq!(
        store.message_meta(&"m1".into()).unwrap(),
        MessageMeta {
            message_id: Some("m1@example.org".into()),
            in_reply_to: vec!["root@example.org".into()],
            references: vec!["root@example.org".into()],
            size: Some(2048),
        }
    );
    // A plain upsert keeps the recorded headers.
    store
        .upsert_message(&server_copy("m1", "a1", "inbox", 1, Flags::default()).message)
        .unwrap();
    assert_eq!(
        store
            .message_meta(&"m1".into())
            .unwrap()
            .message_id
            .as_deref(),
        Some("m1@example.org")
    );
    assert_eq!(
        store
            .find_messages_by_message_id("<m1@example.org>")
            .unwrap()
            .len(),
        1
    );
    // m2 has a blob-less row: nothing to backfill; give it a blob.
    let mut m2 = server_copy("m2", "a1", "inbox", 2, Flags::default()).message;
    m2.blob_hash = Some(nail_store::BlobHash::of(b"raw"));
    store.upsert_message(&m2).unwrap();
    let missing = store.messages_missing_meta(10).unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].0, "m2".into());
}

#[test]
fn sources_record_uid_validity_and_message_id() {
    let store = two_account_store();
    store
        .set_folder_sync_state(
            &"inbox".into(),
            &nail_store::FolderSyncState {
                uidvalidity: Some(77),
                uidnext: Some(10),
                highestmodseq: None,
            },
        )
        .unwrap();
    let mut synced = server_copy("m1", "a1", "inbox", 1, Flags::default());
    synced.meta = Some(MessageMeta {
        message_id: Some("<m1@example.org>".into()),
        ..MessageMeta::default()
    });
    store.merge_messages(&[synced]).unwrap();
    let enqueued = store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m1".into(),
            permanent: true,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    assert_eq!(
        enqueued.op.operation.source(),
        Some(&MessageSource {
            account_id: "a1".into(),
            folder_id: "inbox".into(),
            uid: Some(1),
            uid_validity: Some(77),
            header_message_id: Some("m1@example.org".into()),
            remote_id: None,
        })
    );
    // Old payloads without the new fields still load.
    let old: Operation = serde_json::from_value(json!({"type": "delete_message",
        "message_id": "m1", "permanent": true,
        "source": {"account_id": "a1", "folder_id": "inbox", "uid": 1}}))
    .unwrap();
    assert_eq!(old.source().unwrap().uid_validity, None);
}

#[test]
fn later_ops_learn_where_an_earlier_one_put_the_message() {
    let store = two_account_store();
    let first = store
        .enqueue(&Operation::MoveMessage {
            message_id: "m1".into(),
            target_folder_id: "archive".into(),
            source: None,
        })
        .unwrap();
    // A flag change between the moves has no source and is skipped.
    store.enqueue(&seen("m1", true)).unwrap();
    let second = store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m1".into(),
            permanent: false,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    let third = store
        .enqueue(&Operation::DeleteMessage {
            message_id: "m1".into(),
            permanent: true,
            source: None,
            trash_folder_id: None,
        })
        .unwrap();
    for op in [&second, &third] {
        assert_eq!(op.op.operation.source().unwrap().uid, None);
    }

    // The first move replayed: the message is Archive UID 42 (validity 9).
    let updated = store
        .rebase_later_ops(&"m1".into(), first.op.id, &"archive".into(), 42, Some(9))
        .unwrap();
    assert_eq!(updated, 1, "only the next move / delete, not beyond it");
    let second_now = store.get_op(second.op.id).unwrap();
    let source = second_now.operation.source().unwrap();
    assert_eq!(source.folder_id, "archive".into());
    assert_eq!((source.uid, source.uid_validity), (Some(42), Some(9)));
    assert_eq!(
        store
            .get_op(third.op.id)
            .unwrap()
            .operation
            .source()
            .unwrap()
            .uid,
        None
    );

    // The move to Trash replayed: now the permanent delete knows the UID.
    let updated = store
        .rebase_later_ops(&"m1".into(), second.op.id, &"trash".into(), 7, Some(3))
        .unwrap();
    assert_eq!(updated, 1);
    let source = store.get_op(third.op.id).unwrap().operation;
    assert_eq!(source.source().unwrap().uid, Some(7));

    // A folder that does not match stops the walk; known UIDs are kept.
    assert_eq!(
        store
            .rebase_later_ops(&"m1".into(), first.op.id, &"inbox".into(), 99, None)
            .unwrap(),
        0
    );
}

#[test]
fn update_op_rewrites_a_waiting_operation() {
    let store = two_account_store();
    let queued = store
        .enqueue(&Operation::MoveMessage {
            message_id: "m1".into(),
            target_folder_id: "inbox2".into(),
            source: None,
        })
        .unwrap();
    let source = queued.op.operation.source().cloned();
    let rest = Operation::DeleteMessage {
        message_id: "m1".into(),
        permanent: true,
        source,
        trash_folder_id: None,
    };
    assert!(store.update_op(queued.op.id, &rest).unwrap());
    let now = store.get_op(queued.op.id).unwrap();
    assert_eq!(now.operation, rest);
    assert_eq!(now.state, OpState::Pending);
    // The local row is untouched: still in the target account.
    assert_eq!(
        store.get_message(&"m1".into()).unwrap().summary.folder_id,
        "inbox2".into()
    );
}

#[test]
fn append_keeps_the_local_row_and_needs_its_folder() {
    let store = two_account_store();
    let mut local = message("s1", "a1", "archive", "2026-01-01T00:00:00Z");
    local.uid = None;
    store.upsert_message(&local).unwrap();
    let queued = store
        .enqueue(&Operation::AppendMessage {
            message_id: "s1".into(),
            folder_id: "archive".into(),
        })
        .unwrap();
    assert_eq!(queued.op.account_id, AccountId::from("a1"));
    assert_eq!(queued.op.operation.kind(), "append_message");
    assert!(!queued.op.operation.takes_message_away());
    assert_eq!(queued.changed_folders, [FolderId::from("archive")]);
    assert!(store.get_message(&"s1".into()).is_ok());
    assert!(matches!(
        store.enqueue(&Operation::AppendMessage {
            message_id: "s1".into(),
            folder_id: "inbox".into(),
        }),
        Err(Error::InvalidData(_))
    ));
    // A sync may still update the row (no move / delete is pending).
    let report = store
        .merge_messages(&[server_copy("s1", "a1", "archive", 5, Flags::default())])
        .unwrap();
    assert_eq!(report.written, 1);
}

#[test]
fn clearing_a_folder_keeps_messages_with_waiting_operations() {
    let store = two_account_store();
    // m1 has a pending flag change, m2 none.
    store.enqueue(&seen("m1", true)).unwrap();
    assert_eq!(store.clear_folder(&"inbox".into()).unwrap(), 1);
    assert!(store.get_message(&"m2".into()).unwrap_err().is_not_found());
    let kept = store.get_message(&"m1".into()).unwrap();
    assert_eq!(kept.uid, None, "its UID belonged to the old UIDVALIDITY");
    assert!(kept.summary.flags.seen);
}
