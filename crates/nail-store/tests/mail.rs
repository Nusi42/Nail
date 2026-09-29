//! Accounts, folders and messages CRUD.

mod common;

use common::{account, folder, message, seeded_store};
use nail_api::{AccountKind, Flags, FolderId, FolderRole, MessageId};
use nail_store::{BlobHash, Error, FolderSyncState, Store};
use pretty_assertions::assert_eq;

#[test]
fn accounts_crud_and_order() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.list_accounts().unwrap().is_empty());

    store.upsert_account(&account("b")).unwrap();
    store.upsert_account(&account("a")).unwrap();
    let mut b = account("b");
    b.name = "Renamed".into();
    b.kind = AccountKind::Local;
    store.upsert_account(&b).unwrap();

    let ids: Vec<_> = store
        .list_accounts()
        .unwrap()
        .into_iter()
        .map(|a| a.id.0)
        .collect();
    assert_eq!(ids, ["b", "a"], "insertion order survives updates");
    assert_eq!(store.get_account(&"b".into()).unwrap(), b);
    assert!(store.get_account(&"zz".into()).unwrap_err().is_not_found());

    assert!(store.delete_account(&"a".into()).unwrap());
    assert!(!store.delete_account(&"a".into()).unwrap());
    assert_eq!(store.list_accounts().unwrap(), [b]);
}

#[test]
fn folders_list_parents_first_with_roles() {
    let store = Store::open_in_memory().unwrap();
    store.upsert_account(&account("a1")).unwrap();
    // Parents must exist before their children (foreign key), so the child
    // gets a path that sorts before its parent's: ordering must still put
    // the parent first.
    store
        .upsert_folder(&folder("z", "a1", "Zeta", None, FolderRole::Other))
        .unwrap();
    store
        .upsert_folder(&folder("inbox", "a1", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    store
        .upsert_folder(&folder(
            "child",
            "a1",
            "A-child",
            Some("z"),
            FolderRole::Other,
        ))
        .unwrap();
    store
        .upsert_folder(&folder("sent", "a1", "Sent", None, FolderRole::Sent))
        .unwrap();

    let folders = store.list_folders(&"a1".into()).unwrap();
    let ids: Vec<_> = folders.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["inbox", "sent", "z", "child"]);
    assert_eq!(folders[0].role, FolderRole::Inbox);
    assert_eq!(folders[3].parent_id, Some(FolderId::from("z")));

    let found = store.find_folder_by_path(&"a1".into(), "Sent").unwrap();
    assert_eq!(found.map(|f| f.id), Some("sent".into()));
    assert!(
        store
            .find_folder_by_path(&"a1".into(), "nope")
            .unwrap()
            .is_none()
    );

    assert!(
        store
            .list_folders(&"nope".into())
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn folder_update_keeps_sync_state_and_validates() {
    let store = seeded_store();
    store.upsert_account(&account("a2")).unwrap();

    let state = FolderSyncState {
        uidvalidity: Some(42),
        uidnext: Some(1001),
        highestmodseq: Some(i64::MAX as u64),
    };
    store
        .set_folder_sync_state(&"inbox".into(), &state)
        .unwrap();

    let mut renamed = folder("inbox", "a1", "INBOX", None, FolderRole::Inbox);
    renamed.name = "Posteingang".into();
    store.upsert_folder(&renamed).unwrap();
    assert_eq!(
        store.get_folder(&"inbox".into()).unwrap().name,
        "Posteingang"
    );
    assert_eq!(store.folder_sync_state(&"inbox".into()).unwrap(), state);

    // Too large for SQLite's signed 64-bit integers.
    let too_big = FolderSyncState {
        highestmodseq: Some(u64::MAX),
        ..state
    };
    assert!(matches!(
        store.set_folder_sync_state(&"inbox".into(), &too_big),
        Err(Error::InvalidData(_))
    ));

    // Unknown account.
    assert!(
        store
            .upsert_folder(&folder("x", "nope", "X", None, FolderRole::Other))
            .unwrap_err()
            .is_not_found()
    );
    // Moving a folder to another account.
    assert!(matches!(
        store.upsert_folder(&folder("inbox", "a2", "INBOX", None, FolderRole::Inbox)),
        Err(Error::InvalidData(_))
    ));
    // Parent in another account.
    store
        .upsert_folder(&folder("other", "a2", "Other", None, FolderRole::Other))
        .unwrap();
    assert!(matches!(
        store.upsert_folder(&folder("c", "a1", "C", Some("other"), FolderRole::Other)),
        Err(Error::InvalidData(_))
    ));
    // Self-parent.
    assert!(matches!(
        store.upsert_folder(&folder("c", "a1", "C", Some("c"), FolderRole::Other)),
        Err(Error::InvalidData(_))
    ));
    // Duplicate path within an account.
    assert!(matches!(
        store.upsert_folder(&folder("dup", "a1", "INBOX", None, FolderRole::Other)),
        Err(Error::Sqlite(_))
    ));
    assert_eq!(
        store.folder_sync_state(&"archive".into()).unwrap(),
        FolderSyncState::default()
    );
}

#[test]
fn message_round_trips_all_fields() {
    let store = seeded_store();
    let mut msg = message("m1", "a1", "inbox", "2026-09-29T08:15:00+02:00");
    msg.uid = Some(7);
    msg.blob_hash = Some(BlobHash::of(b"raw"));
    msg.thread_key = Some("root@example.org".into());
    msg.summary.flags = Flags {
        seen: true,
        flagged: true,
        answered: false,
        draft: true,
    };
    msg.summary.has_attachments = true;
    msg.summary.tags = vec!["work".into(), "$label1".into(), "work".into()];
    store.upsert_message(&msg).unwrap();
    let got = store.get_message(&"m1".into()).unwrap();
    // The store threads messages itself and keeps tags sorted and unique.
    assert!(got.summary.thread_id.is_some());
    msg.summary.thread_id = got.summary.thread_id.clone();
    msg.summary.tags = vec!["$label1".into(), "work".into()];
    assert_eq!(got, msg);

    // Malformed mail without From and recipients.
    let mut bare = message("m2", "a1", "inbox", "2026-09-29T08:15:00Z");
    bare.summary.from = None;
    bare.summary.to.clear();
    bare.summary.subject.clear();
    store.upsert_message(&bare).unwrap();
    let got = store.get_message(&"m2".into()).unwrap();
    bare.summary.thread_id = got.summary.thread_id.clone();
    assert_eq!(got, bare);

    // Update in place, including a move to another folder.
    msg.summary.subject = "Changed".into();
    msg.summary.folder_id = "archive".into();
    msg.uid = Some(3);
    store.upsert_message(&msg).unwrap();
    assert_eq!(store.get_message(&"m1".into()).unwrap(), msg);
    assert_eq!(store.get_folder(&"archive".into()).unwrap().total, 1);
    assert_eq!(store.get_folder(&"inbox".into()).unwrap().total, 1);

    assert!(
        store
            .get_message(&"nope".into())
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn message_validation() {
    let store = seeded_store();
    store.upsert_account(&account("a2")).unwrap();
    store
        .upsert_folder(&folder("f2", "a2", "INBOX", None, FolderRole::Inbox))
        .unwrap();

    // Unknown folder.
    assert!(
        store
            .upsert_message(&message("m", "a1", "nope", "2026-01-01T00:00:00Z"))
            .unwrap_err()
            .is_not_found()
    );
    // Account differs from the folder's account.
    assert!(matches!(
        store.upsert_message(&message("m", "a1", "f2", "2026-01-01T00:00:00Z")),
        Err(Error::InvalidData(_))
    ));
    // Same id re-used in another account.
    store
        .upsert_message(&message("m", "a1", "inbox", "2026-01-01T00:00:00Z"))
        .unwrap();
    assert!(matches!(
        store.upsert_message(&message("m", "a2", "f2", "2026-01-01T00:00:00Z")),
        Err(Error::InvalidData(_))
    ));
    // UIDs are unique per folder.
    let mut a = message("u1", "a1", "inbox", "2026-01-01T00:00:00Z");
    a.uid = Some(5);
    let mut b = message("u2", "a1", "inbox", "2026-01-01T00:00:00Z");
    b.uid = Some(5);
    store.upsert_message(&a).unwrap();
    assert!(matches!(store.upsert_message(&b), Err(Error::Sqlite(_))));
    // …but not across folders.
    b.summary.folder_id = "archive".into();
    store.upsert_message(&b).unwrap();
}

#[test]
fn batch_upsert_is_all_or_nothing() {
    let store = seeded_store();
    let good = message("ok", "a1", "inbox", "2026-01-01T00:00:00Z");
    let bad = message("bad", "a1", "nope", "2026-01-01T00:00:00Z");
    assert!(store.upsert_messages(&[good.clone(), bad]).is_err());
    assert_eq!(store.get_folder(&"inbox".into()).unwrap().total, 0);

    let batch: Vec<_> = (0..500)
        .map(|i| message(&format!("m{i:03}"), "a1", "inbox", "2026-01-01T00:00:00Z"))
        .collect();
    store.upsert_messages(&batch).unwrap();
    assert_eq!(store.get_folder(&"inbox".into()).unwrap().total, 500);
}

#[test]
fn list_messages_pages_newest_first() {
    let store = seeded_store();
    // Dates in different zones: sorting must use the instant, not the text.
    let dates = [
        ("m-a", "2026-09-29T10:00:00+02:00"), // 08:00Z
        ("m-b", "2026-09-29T07:30:00-01:00"), // 08:30Z (newest)
        ("m-c", "2026-09-29T08:15:00Z"),      // 08:15Z
        ("m-d", "2026-09-28T23:59:59.500Z"),  // oldest valid
        ("m-e", "not a date"),                // unparseable → last
        ("m-f", "2026-09-29T08:15:00Z"),      // ties m-c → id desc
    ];
    for (id, date) in dates {
        store
            .upsert_message(&message(id, "a1", "inbox", date))
            .unwrap();
    }
    store
        .upsert_message(&message("other", "a1", "archive", "2030-01-01T00:00:00Z"))
        .unwrap();

    let all = store.list_messages(&"inbox".into(), 0, 100).unwrap();
    assert_eq!(all.total, 6);
    let ids: Vec<_> = all.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m-b", "m-f", "m-c", "m-a", "m-d", "m-e"]);

    let page = store.list_messages(&"inbox".into(), 2, 2).unwrap();
    assert_eq!(page.total, 6);
    let ids: Vec<_> = page.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m-c", "m-a"]);

    let past_end = store.list_messages(&"inbox".into(), 10, 5).unwrap();
    assert_eq!(past_end.total, 6);
    assert!(past_end.messages.is_empty());

    let empty = store.list_messages(&"inbox".into(), 0, 0).unwrap();
    assert!(empty.messages.is_empty());

    assert!(
        store
            .list_messages(&"nope".into(), 0, 10)
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn flags_and_unread_counts() {
    let store = seeded_store();
    for id in ["m1", "m2", "m3"] {
        store
            .upsert_message(&message(id, "a1", "inbox", "2026-01-01T00:00:00Z"))
            .unwrap();
    }
    let mut read = message("m4", "a1", "archive", "2026-01-01T00:00:00Z");
    read.summary.flags.seen = true;
    store.upsert_message(&read).unwrap();

    let seen = Flags {
        seen: true,
        ..Flags::default()
    };
    let folder = store.update_flags(&"m2".into(), seen).unwrap();
    assert_eq!(folder, FolderId::from("inbox"));
    assert_eq!(store.get_message(&"m2".into()).unwrap().summary.flags, seen);
    assert!(
        store
            .update_flags(&"nope".into(), seen)
            .unwrap_err()
            .is_not_found()
    );

    let counts = store.unread_counts(&"a1".into()).unwrap();
    let counts: Vec<_> = counts
        .iter()
        .map(|c| (c.folder_id.as_str(), c.unread, c.total))
        .collect();
    assert_eq!(counts, [("archive", 0, 1), ("inbox", 2, 3)]);

    let inbox = store.get_folder(&"inbox".into()).unwrap();
    assert_eq!((inbox.unread, inbox.total), (2, 3));
}

#[test]
fn uid_helpers_for_sync() {
    let store = seeded_store();
    for uid in [30u32, 10, 20] {
        let mut m = message(&format!("m{uid}"), "a1", "inbox", "2026-01-01T00:00:00Z");
        m.uid = Some(uid);
        store.upsert_message(&m).unwrap();
    }
    store
        .upsert_message(&message("local", "a1", "inbox", "2026-01-01T00:00:00Z"))
        .unwrap();

    let uids = store.folder_uids(&"inbox".into()).unwrap();
    assert_eq!(
        uids,
        [
            (10, MessageId::from("m10")),
            (20, MessageId::from("m20")),
            (30, MessageId::from("m30"))
        ]
    );
    let found = store.find_message_by_uid(&"inbox".into(), 20).unwrap();
    assert_eq!(found.map(|m| m.summary.id), Some("m20".into()));
    assert!(
        store
            .find_message_by_uid(&"inbox".into(), 99)
            .unwrap()
            .is_none()
    );

    assert_eq!(
        store
            .delete_messages_by_uid(&"inbox".into(), &[10, 30, 99])
            .unwrap(),
        2
    );
    assert_eq!(store.get_folder(&"inbox".into()).unwrap().total, 2);

    store
        .set_folder_sync_state(
            &"inbox".into(),
            &FolderSyncState {
                uidvalidity: Some(1),
                uidnext: Some(31),
                highestmodseq: None,
            },
        )
        .unwrap();
    assert_eq!(store.clear_folder(&"inbox".into()).unwrap(), 2);
    assert_eq!(store.get_folder(&"inbox".into()).unwrap().total, 0);
    assert_eq!(
        store.folder_sync_state(&"inbox".into()).unwrap(),
        FolderSyncState::default()
    );
    assert!(
        store
            .clear_folder(&"nope".into())
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn deletes_cascade() {
    let store = seeded_store();
    store
        .upsert_folder(&folder(
            "sub",
            "a1",
            "INBOX/Sub",
            Some("inbox"),
            FolderRole::Other,
        ))
        .unwrap();
    let mut m = message("m1", "a1", "inbox", "2026-01-01T00:00:00Z");
    m.blob_hash = Some(BlobHash::of(b"raw"));
    store.upsert_message(&m).unwrap();
    store
        .upsert_message(&message("m2", "a1", "archive", "2026-01-01T00:00:00Z"))
        .unwrap();
    assert!(store.blob_is_referenced(&BlobHash::of(b"raw")).unwrap());

    assert!(store.delete_message(&"m2".into()).unwrap());
    assert!(!store.delete_message(&"m2".into()).unwrap());

    assert!(store.delete_folder(&"inbox".into()).unwrap());
    assert!(store.get_message(&"m1".into()).unwrap_err().is_not_found());
    assert!(!store.blob_is_referenced(&BlobHash::of(b"raw")).unwrap());
    // The child folder survives as a top-level folder.
    assert_eq!(store.get_folder(&"sub".into()).unwrap().parent_id, None);

    assert!(store.delete_account(&"a1".into()).unwrap());
    assert!(store.get_folder(&"sub".into()).unwrap_err().is_not_found());
}

#[test]
fn data_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let (store, blobs) = nail_store::open_data_dir(dir.path()).unwrap();
        store.upsert_account(&account("a1")).unwrap();
        store
            .upsert_folder(&folder("inbox", "a1", "INBOX", None, FolderRole::Inbox))
            .unwrap();
        let mut m = message("m1", "a1", "inbox", "2026-01-01T00:00:00Z");
        m.blob_hash = Some(blobs.put(b"raw message").unwrap());
        store.upsert_message(&m).unwrap();
    }
    assert!(dir.path().join(nail_store::DB_FILE_NAME).is_file());
    assert!(dir.path().join(nail_store::BLOB_DIR_NAME).is_dir());

    let (store, blobs) = nail_store::open_data_dir(dir.path()).unwrap();
    let m = store.get_message(&"m1".into()).unwrap();
    assert_eq!(
        blobs.get(m.blob_hash.as_ref().unwrap()).unwrap(),
        b"raw message"
    );
    assert_eq!(store.list_accounts().unwrap(), [account("a1")]);
}

#[tokio::test]
async fn usable_from_tokio_tasks() {
    let store = seeded_store();
    let handles: Vec<_> = (0..16)
        .map(|i| {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .call(move |s| {
                        s.upsert_message(&message(
                            &format!("m{i}"),
                            "a1",
                            "inbox",
                            "2026-01-01T00:00:00Z",
                        ))
                    })
                    .await
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap().unwrap();
    }
    let page = store
        .call(|s| s.list_messages(&"inbox".into(), 0, 100))
        .await
        .unwrap();
    assert_eq!(page.total, 16);
}

/// Regression: deleting or clearing folders removed the message rows but
/// left their raw mail in the blob store forever.
#[test]
fn unreferenced_blobs_are_collected() {
    let dir = tempfile::tempdir().unwrap();
    let (store, blobs) = nail_store::open_data_dir(dir.path()).unwrap();
    store.upsert_account(&account("a1")).unwrap();
    for (id, path) in [("inbox", "INBOX"), ("archive", "Archive")] {
        store
            .upsert_folder(&folder(id, "a1", path, None, FolderRole::Other))
            .unwrap();
    }
    let only_inbox = blobs.put(b"only in the inbox").unwrap();
    let shared = blobs.put(b"in both folders").unwrap();
    for (id, folder_id, hash) in [
        ("m1", "inbox", &only_inbox),
        ("m2", "inbox", &shared),
        ("m3", "inbox", &shared),
        ("m4", "archive", &shared),
    ] {
        let mut m = message(id, "a1", folder_id, "2026-01-01T00:00:00Z");
        m.blob_hash = Some(hash.clone());
        store.upsert_message(&m).unwrap();
    }

    let mut hashes = store.folder_blob_hashes(&"inbox".into()).unwrap();
    hashes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let mut expected = vec![only_inbox.clone(), shared.clone()];
    expected.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert_eq!(hashes, expected, "distinct hashes");

    assert!(store.delete_folder(&"inbox".into()).unwrap());
    assert_eq!(store.remove_unreferenced_blobs(&blobs, &hashes).unwrap(), 1);
    assert!(!blobs.contains(&only_inbox));
    assert!(blobs.contains(&shared), "still referenced by the archive");

    let hashes = store.folder_blob_hashes(&"archive".into()).unwrap();
    store.clear_folder(&"archive".into()).unwrap();
    assert_eq!(store.remove_unreferenced_blobs(&blobs, &hashes).unwrap(), 1);
    assert!(!blobs.contains(&shared));
    // Removing again is a no-op.
    assert_eq!(store.remove_unreferenced_blobs(&blobs, &hashes).unwrap(), 0);
}
