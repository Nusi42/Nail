//! Upgrading a database written by the M0 build (schema v1) to the current
//! schema: data survives, threads are backfilled from the M0 thread keys,
//! queued operations keep working.
#![cfg(test)]

use nail_api::{Flags, FolderId, MessageId};
use nail_store::{
    DB_FILE_NAME, FolderSyncState, MessageMeta, OpState, SCHEMA_VERSION, Store, StoredMessage,
    SyncedMessage,
};
use pretty_assertions::assert_eq;

const V1_FIXTURE: &str = include_str!("fixtures/store-v1.sql");

fn v1_database() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DB_FILE_NAME);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(V1_FIXTURE).unwrap();
    drop(conn);
    (dir, path)
}

#[test]
fn upgrades_a_v1_database() {
    let (_dir, path) = v1_database();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    const { assert!(SCHEMA_VERSION >= 2) };

    // Accounts, folders and sync state are intact.
    let accounts = store.list_accounts().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].name, "Work");
    assert_eq!(
        store.folder_sync_state(&"inbox".into()).unwrap(),
        FolderSyncState {
            uidvalidity: Some(7),
            uidnext: Some(100),
            highestmodseq: Some(5000),
        }
    );
    let inbox = store.get_folder(&"inbox".into()).unwrap();
    assert_eq!((inbox.total, inbox.unread), (2, 1));

    // Messages are intact and threaded by their M0 thread key.
    let m2 = store.get_message(&"m2".into()).unwrap();
    assert_eq!(m2.summary.subject, "Re: Plans");
    assert!(m2.summary.flags.flagged && m2.summary.has_attachments);
    assert_eq!(m2.summary.to.len(), 1);
    assert_eq!(m2.thread_key.as_deref(), Some("root@x"));
    assert!(m2.summary.tags.is_empty());
    let t1 = store.thread_of(&"m1".into()).unwrap();
    assert_eq!(store.thread_of(&"m2".into()).unwrap(), t1);
    assert_ne!(store.thread_of(&"m3".into()).unwrap(), t1);
    assert_ne!(
        store.thread_of(&"m4".into()).unwrap(),
        store.thread_of(&"m3".into()).unwrap()
    );
    let page = store
        .list_threads(&[FolderId::from("inbox")], 0, 10)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.threads[0].subject, "Plans");
    assert_eq!(page.threads[0].message_count, 2);

    // Headers are unknown after the upgrade; the core can backfill them
    // from blobs (none here) and messages keep their thread.
    assert_eq!(
        store.message_meta(&"m1".into()).unwrap(),
        MessageMeta::default()
    );

    // The queue: ids continue, the pending op is still pending and the
    // sync merge knows which message it is about.
    let ops = store.list_ops(None).unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].state, OpState::Pending);
    assert_eq!(
        store.pending_ops_for_message(&"m2".into()).unwrap().len(),
        1
    );
    let mut server = StoredMessage::new(m2.summary.clone());
    server.uid = m2.uid;
    server.summary.flags = Flags::default();
    let report = store
        .merge_messages(&[SyncedMessage {
            message: server,
            meta: None,
        }])
        .unwrap();
    assert_eq!(report.kept_local, [MessageId::from("m2")]);
    assert!(store.get_message(&"m2".into()).unwrap().summary.flags.seen);

    // New v2 features work on the upgraded database.
    let draft = nail_api::Draft {
        subject: "Hi".into(),
        ..nail_api::Draft::new("ada@example.org")
    };
    let id = store.save_draft(&draft).unwrap();
    store.schedule_send(&id, &"a1".into(), 0).unwrap();
    assert_eq!(store.list_outbox().unwrap().len(), 1);

    // Reopening does not migrate again.
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(store.thread_of(&"m2".into()).unwrap(), t1);
}

#[test]
fn upgraded_threads_match_a_fresh_rethread() {
    let (_dir, path) = v1_database();
    let store = Store::open(&path).unwrap();
    let ids: Vec<MessageId> = ["m1", "m2", "m3", "m4"].map(MessageId::from).to_vec();
    let before = store.thread_ids(&ids).unwrap();
    store.rethread_all().unwrap();
    assert_eq!(store.thread_ids(&ids).unwrap(), before);
}

#[test]
fn a_newer_database_is_refused() {
    let (_dir, path) = v1_database();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        [SCHEMA_VERSION + 5],
    )
    .unwrap();
    drop(conn);
    let err = Store::open(&path).unwrap_err();
    assert!(
        matches!(err, nail_store::Error::SchemaTooNew { found, .. } if found == SCHEMA_VERSION + 5),
        "{err}"
    );
}
