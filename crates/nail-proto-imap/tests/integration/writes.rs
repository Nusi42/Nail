//! Writes against the mock server: flags (STORE, UNCHANGEDSINCE), MOVE and
//! its fallbacks, COPY, expunge, APPEND (UIDPLUS, LITERAL+, limits) and
//! folder CREATE / RENAME / DELETE.

use crate::common::*;
use nail_api::{ErrorCode, FlagChange, FolderRole};
use nail_proto_imap::{
    AppendResult, ExpungeMethod, Flag, ImapError, MoveMethod, StoreMode, UidRange, folder_path,
};
use nail_testkit::MockCapabilities;

#[tokio::test]
async fn store_adds_removes_and_replaces_flags_and_keywords() {
    let server = modern_server().await;
    let mut client = login(&server).await;

    let result = client
        .store_flags(
            "INBOX",
            &[1, 3],
            StoreMode::Add,
            &[Flag::Flagged, Flag::keyword("$Label1").unwrap()],
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        server_flags(&server, "INBOX", 1),
        ["$Label1", "\\Flagged", "\\Seen"]
    );
    assert_eq!(server_flags(&server, "INBOX", 3), ["$Label1", "\\Flagged"]);
    let reported: Vec<u32> = result.updated.iter().map(|u| u.uid).collect();
    assert_eq!(reported, [1, 3]);
    assert!(result.updated[0].flags.flags.flagged);
    assert_eq!(result.updated[0].flags.keywords, ["$Label1"]);
    assert!(result.updated.iter().all(|u| u.modseq.is_some()));
    assert!(result.conflicts.is_empty(), "{:?}", result.conflicts);

    client
        .store_flags("INBOX", &[1], StoreMode::Remove, &[Flag::Seen], None)
        .await
        .unwrap();
    assert_eq!(server_flags(&server, "INBOX", 1), ["$Label1", "\\Flagged"]);

    client
        .store_flags("INBOX", &[3], StoreMode::Replace, &[Flag::Answered], None)
        .await
        .unwrap();
    assert_eq!(server_flags(&server, "INBOX", 3), ["\\Answered"]);
    assert!(server.commands().iter().any(|c| c == "UID STORE"));
    // Our own STORE results are not reported as changes by others.
    assert_eq!(client.take_changes().len(), 0);
}

#[tokio::test]
async fn api_flag_changes_become_add_and_remove_stores() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let change = FlagChange {
        seen: Some(false),
        flagged: Some(true),
        add_tags: vec!["work".into()],
        ..FlagChange::default()
    };
    let result = client
        .apply_flag_change("INBOX", &[1], &change)
        .await
        .unwrap();
    assert_eq!(server_flags(&server, "INBOX", 1), ["\\Flagged", "work"]);
    assert_eq!(result.updated.len(), 1);
    assert!(!result.updated[0].flags.flags.seen);

    let remove = FlagChange {
        remove_tags: vec!["work".into()],
        ..FlagChange::default()
    };
    client
        .apply_flag_change("INBOX", &[1], &remove)
        .await
        .unwrap();
    assert_eq!(server_flags(&server, "INBOX", 1), ["\\Flagged"]);

    let bad = FlagChange {
        add_tags: vec!["two words".into()],
        ..FlagChange::default()
    };
    let before = server.commands().len();
    let err = client
        .apply_flag_change("INBOX", &[1], &bad)
        .await
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRequest);
    assert_eq!(server.commands().len(), before, "nothing was sent");
}

#[tokio::test]
async fn conditional_store_reports_conflicts() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let modseq = client
        .select("INBOX")
        .await
        .unwrap()
        .highest_modseq
        .unwrap();

    // Another client flags message 2 after we read the mailbox.
    server.store().add_flags("INBOX", 2, &["$Important"]);
    let result = client
        .store_flags(
            "INBOX",
            &[1, 2],
            StoreMode::Add,
            &[Flag::Answered],
            Some(modseq),
        )
        .await
        .unwrap();
    assert_eq!(result.conflicts, [2]);
    assert!(server_flags(&server, "INBOX", 1).contains(&"\\Answered".to_owned()));
    assert!(!server_flags(&server, "INBOX", 2).contains(&"\\Answered".to_owned()));

    // Without CONDSTORE the precondition cannot be honoured.
    let server = bare_server().await;
    let mut client = login(&server).await;
    let err = client
        .store_flags("INBOX", &[1], StoreMode::Add, &[Flag::Seen], Some(1))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::Unsupported(_)), "{err:?}");
}

#[tokio::test]
async fn invalid_uids_and_flags_are_refused_locally() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    for uids in [&[][..], &[0][..]] {
        let err = client
            .store_flags("INBOX", uids, StoreMode::Add, &[Flag::Seen], None)
            .await
            .unwrap_err();
        assert!(matches!(err, ImapError::InvalidArgument(_)));
    }
    let err = client
        .store_flags(
            "INBOX",
            &[1],
            StoreMode::Add,
            &[Flag::Keyword("a)b".into())],
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::InvalidArgument(_)));
}

#[tokio::test]
async fn move_uses_move_and_reports_the_new_uids() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let result = client
        .move_messages("INBOX", &[1, 3], "Archive")
        .await
        .unwrap();
    assert_eq!(result.method, MoveMethod::Move);
    let archive = server.store().mailbox("Archive").unwrap();
    assert_eq!(result.copy.uid_validity, Some(archive.uid_validity));
    assert_eq!(result.copy.mapping, [(1, 1), (3, 2)]);
    assert_eq!(server_uids(&server, "INBOX"), [2]);
    assert_eq!(server_uids(&server, "Archive"), [1, 2]);
    assert_eq!(server_flags(&server, "Archive", 1), ["\\Seen"]);
    assert!(server.commands().iter().any(|c| c == "UID MOVE"));
    // Our own expunges are not reported as foreign changes.
    assert_eq!(client.take_changes().len(), 0);
}

#[tokio::test]
async fn move_falls_back_to_copy_store_and_uid_expunge() {
    let server = server_with(MockCapabilities {
        move_: false,
        ..MockCapabilities::default()
    })
    .await;
    let mut client = login(&server).await;
    let result = client
        .move_messages("INBOX", &[2], "Archive")
        .await
        .unwrap();
    assert_eq!(result.method, MoveMethod::CopyAndExpunge);
    assert_eq!(result.copy.mapping, [(2, 1)]);
    assert_eq!(server_uids(&server, "INBOX"), [1, 3]);
    assert_eq!(server_flags(&server, "Archive", 1), ["\\Flagged"]);
    let commands = server.commands();
    for expected in ["UID COPY", "UID STORE", "UID EXPUNGE"] {
        assert!(commands.iter().any(|c| c == expected), "{commands:?}");
    }
    assert!(!commands.iter().any(|c| c == "UID MOVE"));
}

#[tokio::test]
async fn move_without_uidplus_expunges_only_when_safe() {
    let server = bare_server().await;
    let mut client = login(&server).await;
    let result = client
        .move_messages("INBOX", &[1], "Archive")
        .await
        .unwrap();
    assert_eq!(result.method, MoveMethod::CopyAndExpunge);
    assert!(result.copy.mapping.is_empty(), "no COPYUID without UIDPLUS");
    assert_eq!(server_uids(&server, "INBOX"), [2, 3]);
    assert!(server.commands().iter().any(|c| c == "EXPUNGE"));

    // Another message is marked \Deleted by someone else: a plain EXPUNGE
    // would remove it too, so the moved message is only marked.
    server.store().add_flags("INBOX", 3, &["\\Deleted"]);
    let result = client
        .move_messages("INBOX", &[2], "Archive")
        .await
        .unwrap();
    assert_eq!(result.method, MoveMethod::CopyAndMarkDeleted);
    assert_eq!(server_uids(&server, "INBOX"), [2, 3]);
    assert!(server_flags(&server, "INBOX", 2).contains(&"\\Deleted".to_owned()));
    assert_eq!(server_uids(&server, "Archive").len(), 2);
}

#[tokio::test]
async fn copy_reports_copyuid_and_missing_targets() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let result = client
        .copy_messages("INBOX", &[2, 3], "Trash")
        .await
        .unwrap();
    assert_eq!(result.mapping, [(2, 1), (3, 2)]);
    assert_eq!(server_uids(&server, "INBOX"), [1, 2, 3]);

    let err = client
        .copy_messages("INBOX", &[1], "Nowhere")
        .await
        .unwrap_err();
    assert_eq!(err.response_code(), Some("TRYCREATE"));
    assert_eq!(err.code(), ErrorCode::NotFound);
}

#[tokio::test]
async fn expunge_removes_exactly_the_given_messages() {
    let server = modern_server().await;
    // Another message already carries \Deleted; UID EXPUNGE leaves it alone.
    server.store().add_flags("INBOX", 3, &["\\Deleted"]);
    let mut client = login(&server).await;
    let method = client.expunge_messages("INBOX", &[1]).await.unwrap();
    assert_eq!(method, ExpungeMethod::UidExpunge);
    assert_eq!(server_uids(&server, "INBOX"), [2, 3]);

    // Without UIDPLUS and nothing else deleted, a plain EXPUNGE is safe.
    let server = bare_server().await;
    let mut client = login(&server).await;
    assert_eq!(
        client.expunge_messages("INBOX", &[2]).await.unwrap(),
        ExpungeMethod::Expunge
    );
    assert_eq!(server_uids(&server, "INBOX"), [1, 3]);
}

#[tokio::test]
async fn append_returns_the_new_uid_with_uidplus() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let raw = message("draft");
    let result = client
        .append(
            "Archive",
            &raw,
            &[Flag::Seen, Flag::Draft],
            Some("28-Sep-2026 08:15:00 +0200"),
        )
        .await
        .unwrap();
    let archive = server.store().mailbox("Archive").unwrap();
    assert_eq!(
        result,
        AppendResult {
            uid_validity: Some(archive.uid_validity),
            uid: Some(1),
        }
    );
    assert_eq!(
        archive.messages[0].internal_date,
        "28-Sep-2026 08:15:00 +0200"
    );
    assert_eq!(server_flags(&server, "Archive", 1), ["\\Draft", "\\Seen"]);
    assert_eq!(archive.messages[0].raw.as_slice(), raw.as_slice());

    // The appended message reads back.
    client.examine("Archive").await.unwrap();
    let fetched = client.fetch_range(UidRange::ALL).await.unwrap();
    assert_eq!(fetched[0].raw, raw);
    assert!(fetched[0].flags.draft);
    assert_eq!(
        fetched[0].internal_date.as_deref(),
        Some("2026-09-28T08:15:00+02:00")
    );

    let err = client
        .append("Archive", &raw, &[], Some("yesterday"))
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::InvalidArgument(_)));
}

#[tokio::test]
async fn append_without_uidplus_or_literal_plus() {
    let server = bare_server().await;
    let mut client = login(&server).await;
    let result = client
        .append("INBOX", &message("sync-literal"), &[], None)
        .await
        .unwrap();
    assert_eq!(result, AppendResult::default());
    assert_eq!(server_uids(&server, "INBOX"), [1, 2, 3, 4]);
    // The connection is still in step after the synchronizing literal.
    client.noop().await.unwrap();

    let err = client
        .append("Nowhere", &message("x"), &[], None)
        .await
        .unwrap_err();
    assert_eq!(err.response_code(), Some("TRYCREATE"));
    client.noop().await.unwrap();
}

#[tokio::test]
async fn append_limits_are_enforced_before_and_by_the_server() {
    // Advertised APPENDLIMIT: checked locally, nothing is uploaded.
    let server = server_with(MockCapabilities {
        append_limit: Some(64),
        ..MockCapabilities::default()
    })
    .await;
    let mut client = login(&server).await;
    assert_eq!(client.append_limit(), Some(64));
    let err = client
        .append("INBOX", &message("too big"), &[], None)
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::TooLarge(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::TooLarge);
    assert!(!server.commands().iter().any(|c| c == "APPEND"));

    // Hidden limit, synchronizing literal: the server refuses instead of
    // sending a continuation, and the connection stays usable.
    let server = server_with(MockCapabilities {
        append_limit: Some(64),
        advertise_append_limit: false,
        ..MockCapabilities::imap4rev1()
    })
    .await;
    let mut client = login(&server).await;
    assert_eq!(client.append_limit(), None);
    let err = client
        .append("INBOX", &message("too big"), &[], None)
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::TooLarge(_)), "{err:?}");
    client.noop().await.unwrap();

    // Hidden limit with LITERAL+: the upload happens, then TOOBIG.
    let server = server_with(MockCapabilities {
        append_limit: Some(64),
        advertise_append_limit: false,
        ..MockCapabilities::default()
    })
    .await;
    let mut client = login(&server).await;
    let err = client
        .append("INBOX", &message("too big"), &[], None)
        .await
        .unwrap_err();
    assert!(matches!(err, ImapError::TooLarge(_)), "{err:?}");
    client.noop().await.unwrap();
}

#[tokio::test]
async fn folders_are_created_renamed_and_deleted() {
    let server = modern_server().await;
    let mut client = login(&server).await;

    let path = folder_path(None, Some("/"), "Entwürfe 2026").unwrap();
    assert_eq!(path, "Entw&APw-rfe 2026");
    client.create_folder(&path).await.unwrap();
    let child = folder_path(Some(&path), Some("/"), "Q3").unwrap();
    client.create_folder(&child).await.unwrap();
    let folders = client.list_folders().await.unwrap();
    let created = folders.iter().find(|f| f.path == path).unwrap();
    assert_eq!(created.name, "Entwürfe 2026");
    assert_eq!(created.role, FolderRole::Other);

    let err = client.create_folder(&path).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::Conflict);

    // Renaming the selected folder works (it is left first) and takes the
    // children along.
    client.select(&path).await.unwrap();
    client.rename_folder(&path, "Drafts 2026").await.unwrap();
    assert_eq!(client.selected_mailbox(), None);
    let names: Vec<String> = client
        .list_folders()
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert!(names.contains(&"Drafts 2026".to_owned()));
    assert!(names.contains(&"Drafts 2026/Q3".to_owned()));
    assert!(!names.contains(&path));

    client.delete_folder("Drafts 2026/Q3").await.unwrap();
    client.delete_folder("Drafts 2026").await.unwrap();
    let err = client.delete_folder("Drafts 2026").await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::NotFound);
    client.set_subscribed("Archive", true).await.unwrap();
    assert!(
        server
            .store()
            .mailboxes()
            .iter()
            .all(|m| !m.name.starts_with("Drafts"))
    );
}

#[tokio::test]
async fn messages_are_found_by_message_id_and_uid_validity_is_reported() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let status = client.select_for_write("INBOX").await.unwrap();
    assert_eq!(
        status.uid_validity,
        server.store().mailbox("INBOX").unwrap().uid_validity
    );
    assert_eq!(
        client
            .find_by_message_id("INBOX", "two@example.org")
            .await
            .unwrap(),
        [2]
    );
    // Brackets are optional; only the exact id matches (not a substring).
    assert_eq!(
        client
            .find_by_message_id("INBOX", "<three@example.org>")
            .await
            .unwrap(),
        [3]
    );
    assert_eq!(
        client
            .find_by_message_id("INBOX", "wo@example.org")
            .await
            .unwrap()
            .len(),
        0
    );
    assert!(
        client
            .find_by_message_id("INBOX", " <> ")
            .await
            .unwrap()
            .is_empty(),
        "{:?}",
        client.find_by_message_id("INBOX", " <> ").await.unwrap()
    );

    // After a rebuild the UIDs change and so does the UIDVALIDITY.
    server.store().expunge("INBOX", 1);
    let new_validity = server.store().reset_uid_validity("INBOX").unwrap();
    let mut client = login(&server).await;
    let status = client.select_for_write("INBOX").await.unwrap();
    assert_eq!(status.uid_validity, new_validity);
    assert_eq!(
        client
            .find_by_message_id("INBOX", "two@example.org")
            .await
            .unwrap(),
        [1]
    );
}
