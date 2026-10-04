//! Body-less messages and on-demand bodies (M2c WP53): headers-only merges,
//! attaching / detaching bodies, fetch targets and the MCP settings.
#![cfg(test)]

mod common;

use common::{message, seeded_store};
use nail_api::MessageId;
use nail_api::mcp::McpSettings;
use nail_api::sync::BodyState;
use nail_store::{
    BlobHash, BlobStore, BodyFetchTarget, MCP_SETTINGS_KEY, MessageMeta, Operation, Store,
    StoredBody, SyncedMessage,
};
use pretty_assertions::assert_eq;

const HEADERS: &[u8] = b"Subject: hi\r\nFrom: ada@example.org\r\n\r\n";
const FULL: &[u8] = b"Subject: hi\r\nFrom: ada@example.org\r\n\r\nThe body.\r\n";

fn synced(id: &str, date: &str, uid: u32, blob: &BlobHash, size: u64) -> SyncedMessage {
    let mut m = message(id, "a1", "inbox", date);
    m.uid = Some(uid);
    m.blob_hash = Some(blob.clone());
    SyncedMessage {
        message: m,
        meta: Some(MessageMeta {
            message_id: Some(format!("{id}@example.org")),
            size: Some(size),
            ..MessageMeta::default()
        }),
    }
}

fn state(store: &Store, id: &str) -> BodyState {
    store.body_state(&MessageId::from(id)).unwrap()
}

fn blob_of(store: &Store, id: &str) -> BlobHash {
    store
        .get_message(&MessageId::from(id))
        .unwrap()
        .blob_hash
        .unwrap()
}

#[test]
fn headers_only_merges_mark_rows_and_keep_downloaded_bodies() {
    let store = seeded_store();
    let headers = BlobHash::of(HEADERS);
    let full = BlobHash::of(FULL);
    let report = store
        .merge_with_body_state(
            &[
                synced("m1", "2026-09-01T10:00:00Z", 1, &headers, 40),
                synced("m2", "2026-09-02T10:00:00Z", 2, &headers, 40),
            ],
            BodyState::HeadersOnly,
        )
        .unwrap();
    assert_eq!(report.written, 2);
    assert_eq!(state(&store, "m1"), BodyState::HeadersOnly);
    assert!(
        store
            .get_message(&"m1".into())
            .unwrap()
            .summary
            .headers_only
    );
    assert_eq!(store.headers_only_count(&"inbox".into()).unwrap(), 2);

    // m1 is opened: its body is fetched on demand.
    let old = store.attach_body(&"m1".into(), &full, 55).unwrap();
    assert_eq!(old, Some(headers.clone()), "the header blob can go");
    assert_eq!(state(&store, "m1"), BodyState::OnDemand);
    assert!(
        !store
            .get_message(&"m1".into())
            .unwrap()
            .summary
            .headers_only
    );
    assert_eq!(store.message_meta(&"m1".into()).unwrap().size, Some(55));
    // Attaching the same blob again changes nothing to collect.
    assert_eq!(store.attach_body(&"m1".into(), &full, 55).unwrap(), None);

    // A resync of the headers-only folder keeps the downloaded body and
    // its size.
    store
        .merge_with_body_state(
            &[synced("m1", "2026-09-01T10:00:00Z", 1, &headers, 40)],
            BodyState::HeadersOnly,
        )
        .unwrap();
    assert_eq!(state(&store, "m1"), BodyState::OnDemand);
    assert_eq!(blob_of(&store, "m1"), full);
    assert_eq!(store.message_meta(&"m1".into()).unwrap().size, Some(55));
    assert_eq!(state(&store, "m2"), BodyState::HeadersOnly);

    // The policy changes to full bodies: a full merge makes rows synced.
    store
        .merge_with_body_state(
            &[synced("m2", "2026-09-02T10:00:00Z", 2, &full, 55)],
            BodyState::Synced,
        )
        .unwrap();
    assert_eq!(state(&store, "m2"), BodyState::Synced);
    assert_eq!(blob_of(&store, "m2"), full);
    assert_eq!(store.headers_only_count(&"inbox".into()).unwrap(), 0);

    assert!(
        store
            .merge_with_body_state(&[], BodyState::OnDemand)
            .is_err(),
        "on-demand bodies are attached, not merged"
    );
}

#[test]
fn pending_moves_still_win_over_headers_only_merges() {
    let store = seeded_store();
    let headers = BlobHash::of(HEADERS);
    store
        .merge_with_body_state(
            &[synced("m1", "2026-09-01T10:00:00Z", 1, &headers, 40)],
            BodyState::HeadersOnly,
        )
        .unwrap();
    store
        .enqueue(&Operation::MoveMessage {
            message_id: "m1".into(),
            target_folder_id: "archive".into(),
            source: None,
        })
        .unwrap();
    let report = store
        .merge_with_body_state(
            &[synced("m1", "2026-09-01T10:00:00Z", 1, &headers, 40)],
            BodyState::HeadersOnly,
        )
        .unwrap();
    assert_eq!(report.skipped, [MessageId::from("m1")]);
}

#[test]
fn fetch_targets_name_folder_path_and_uid() {
    let store = seeded_store();
    let headers = BlobHash::of(HEADERS);
    store
        .merge_with_body_state(
            &[
                synced("m1", "2026-09-01T10:00:00Z", 7, &headers, 40),
                synced("m2", "2026-09-02T10:00:00Z", 8, &headers, 40),
            ],
            BodyState::HeadersOnly,
        )
        .unwrap();
    store
        .attach_body(&"m2".into(), &BlobHash::of(FULL), 55)
        .unwrap();
    let targets = store
        .body_fetch_targets(&["m2".into(), "m1".into(), "missing".into()])
        .unwrap();
    assert_eq!(
        targets,
        [BodyFetchTarget {
            message_id: "m1".into(),
            account_id: "a1".into(),
            folder_id: "inbox".into(),
            folder_path: "INBOX".into(),
            uid: Some(7),
            state: BodyState::HeadersOnly,
        }]
    );
}

#[test]
fn aged_bodies_can_be_detached_again() {
    let store = seeded_store();
    let blobs_dir = tempfile::tempdir().unwrap();
    let blobs = BlobStore::open(blobs_dir.path()).unwrap();
    let full_old = blobs.put(FULL).unwrap();
    let full_new = blobs.put(b"Subject: new\r\n\r\nNewer body.\r\n").unwrap();
    let headers = blobs.put(HEADERS).unwrap();
    store
        .merge_with_body_state(
            &[
                synced("old", "2026-01-01T10:00:00Z", 1, &full_old, 55),
                synced("new", "2026-09-20T10:00:00Z", 2, &full_new, 30),
            ],
            BodyState::Synced,
        )
        .unwrap();
    let cutoff = 1_780_000_000; // 2026-05-28
    let aged = store.bodies_before(&"inbox".into(), cutoff, 10).unwrap();
    assert_eq!(
        aged,
        [StoredBody {
            message_id: "old".into(),
            blob_hash: full_old.clone(),
            state: BodyState::Synced,
        }]
    );
    let dropped = store.detach_body(&"old".into(), &headers).unwrap();
    assert_eq!(dropped, Some(full_old.clone()));
    assert_eq!(state(&store, "old"), BodyState::HeadersOnly);
    assert_eq!(blob_of(&store, "old"), headers);
    // Detaching twice is a no-op; the full blob is no longer referenced.
    assert_eq!(store.detach_body(&"old".into(), &headers).unwrap(), None);
    assert!(!store.blob_is_referenced(&full_old).unwrap());
    assert!(
        store
            .bodies_before(&"inbox".into(), cutoff, 10)
            .unwrap()
            .is_empty()
    );
    assert!(store.detach_body(&"missing".into(), &headers).is_err());
    assert!(store.attach_body(&"missing".into(), &headers, 1).is_err());
}

#[test]
fn mcp_settings_default_and_round_trip() {
    let store = seeded_store();
    assert_eq!(store.mcp_settings().unwrap(), McpSettings::default());
    store
        .set_setting(MCP_SETTINGS_KEY, &McpSettings { port: Some(4799) })
        .unwrap();
    assert_eq!(store.mcp_settings().unwrap().port, Some(4799));
}

#[test]
fn merges_with_remote_extras_in_one_transaction() {
    use nail_store::exchange::{ExchangeItemRecord, RemoteExtras};
    use std::collections::BTreeMap;

    let store = seeded_store();
    let full = BlobHash::of(FULL);
    let record = ExchangeItemRecord {
        change_key: Some("ck-1".to_owned()),
        conversation_id: Some("conv-1".to_owned()),
        ..ExchangeItemRecord::default()
    };
    let extras = BTreeMap::from([(
        MessageId::from("m1"),
        RemoteExtras {
            remote_id: Some("AAMk-1".to_owned()),
            exchange: Some(record.clone()),
            gmail: None,
        },
    )]);
    let report = store
        .merge_with_remote(
            &[
                synced("m1", "2026-09-01T10:00:00Z", 1, &full, 50),
                synced("m2", "2026-09-02T10:00:00Z", 2, &full, 50),
            ],
            BodyState::Synced,
            &extras,
        )
        .unwrap();
    assert_eq!(report.written, 2);
    assert_eq!(
        store.message_remote_id(&"m1".into()).unwrap().as_deref(),
        Some("AAMk-1")
    );
    assert_eq!(store.exchange_item(&"m1".into()).unwrap(), Some(record));
    // Messages without extras keep theirs.
    assert_eq!(store.message_remote_id(&"m2".into()).unwrap(), None);
    assert_eq!(store.exchange_item(&"m2".into()).unwrap(), None);

    // Extras for messages outside the batch are ignored.
    let stray = BTreeMap::from([(
        MessageId::from("m3"),
        RemoteExtras {
            remote_id: Some("x".to_owned()),
            ..RemoteExtras::default()
        },
    )]);
    store
        .merge_with_remote(
            &[synced("m4", "2026-09-03T10:00:00Z", 4, &full, 50)],
            BodyState::Synced,
            &stray,
        )
        .unwrap();
    assert!(store.message_exists(&"m4".into()).unwrap());
    assert!(!store.message_exists(&"m3".into()).unwrap());
}
