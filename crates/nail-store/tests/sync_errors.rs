//! Schema v19 (ADRs 0190 / 0191): stored sync errors and skipped messages.
#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared fixtures")]
mod common;

use common::seeded_store;
use nail_api::connlog::{ErrorDetails, ErrorStep, WireProtocol};
use nail_api::{AccountId, ErrorCode, FolderId};

fn details(step: ErrorStep, text: &str) -> ErrorDetails {
    ErrorDetails {
        code: ErrorCode::NotFound,
        protocol: Some(WireProtocol::Imap),
        step,
        folder: Some("INBOX".into()),
        command: Some("A0012 UID FETCH 151:200 (UID FLAGS)".into()),
        server_text: Some(text.into()),
        detail: "server refused".into(),
        at: "2026-10-08T09:00:00Z".into(),
        log_seq: Some(42),
    }
}

#[test]
fn errors_per_account_and_folder_survive_until_cleared() {
    let store = seeded_store();
    let account: AccountId = "a1".into();
    let inbox: FolderId = "inbox".into();
    store
        .set_sync_error(&account, None, &details(ErrorStep::Login, "NO login"))
        .unwrap();
    store
        .set_sync_error(&account, Some(&inbox), &details(ErrorStep::Fetch, "NO a"))
        .unwrap();
    store
        .set_sync_error(&account, Some(&inbox), &details(ErrorStep::Fetch, "NO b"))
        .unwrap();
    let errors = store.sync_errors(Some(&account)).unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].folder_id, None);
    assert_eq!(errors[1].folder_id.as_ref(), Some(&inbox));
    assert_eq!(errors[1].details.server_text.as_deref(), Some("NO b"));
    assert_eq!(store.sync_errors(None).unwrap().len(), 2);
    assert_eq!(store.sync_errors(Some(&"other".into())).unwrap(), []);

    assert!(store.clear_sync_error(&account, Some(&inbox)).unwrap());
    assert!(!store.clear_sync_error(&account, Some(&inbox)).unwrap());
    assert_eq!(store.sync_errors(None).unwrap().len(), 1);
    // A deleted folder takes its error along.
    store
        .set_sync_error(&account, Some(&inbox), &details(ErrorStep::Select, "NO"))
        .unwrap();
    assert!(store.delete_folder(&inbox).unwrap());
    assert_eq!(store.sync_errors(None).unwrap().len(), 1);
}

#[test]
fn skipped_messages_count_attempts_and_go_with_their_folder() {
    let store = seeded_store();
    let inbox: FolderId = "inbox".into();
    let archive: FolderId = "archive".into();
    for uid in [151, 7, 9] {
        store
            .record_skipped(&inbox, uid, &details(ErrorStep::Fetch, "NO [SERVERBUG]"))
            .unwrap();
    }
    store
        .record_skipped(&inbox, 7, &details(ErrorStep::Fetch, "again"))
        .unwrap();
    store
        .record_skipped(&archive, 3, &details(ErrorStep::Fetch, "x"))
        .unwrap();
    let skipped = store.skipped_messages(Some(&"a1".into())).unwrap();
    assert_eq!(skipped.len(), 4);
    let seven = skipped.iter().find(|s| s.uid == 7).unwrap();
    assert_eq!(seven.attempts, 2);
    assert_eq!(seven.details.server_text.as_deref(), Some("again"));
    // Least tried first, returned in UID order.
    assert_eq!(store.skipped_uids(&inbox, 2).unwrap(), [9, 151]);
    assert_eq!(store.remove_skipped(&inbox, &[9, 1000]).unwrap(), 1);
    store.clear_skipped(&inbox).unwrap();
    assert_eq!(store.skipped_uids(&inbox, 10).unwrap(), [0u32; 0]);
    assert!(store.delete_folder(&archive).unwrap());
    assert_eq!(store.skipped_messages(None).unwrap(), []);
}
