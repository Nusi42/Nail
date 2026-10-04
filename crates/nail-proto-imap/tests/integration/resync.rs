//! Flag and expunge resynchronization: QRESYNC, CONDSTORE and the full
//! flag-diff fallback must all find the same changes.

use std::collections::BTreeMap;

use crate::common::*;
use nail_proto_imap::{FlagResync, ImapClient, MessageFlags, ResyncMethod, ResyncState, UidRange};
use nail_testkit::{MockCapabilities, MockImapServer, NO_FLAGS};

/// Local knowledge after a first full fetch: flags per UID and the state.
async fn snapshot(client: &mut ImapClient) -> (BTreeMap<u32, MessageFlags>, ResyncState) {
    let status = client.examine("INBOX").await.unwrap();
    let messages = client.fetch_range(UidRange::ALL).await.unwrap();
    let local = messages
        .into_iter()
        .map(|m| {
            let mut atoms: Vec<String> = m.keywords.clone();
            for (on, name) in [
                (m.flags.seen, "\\Seen"),
                (m.flags.flagged, "\\Flagged"),
                (m.flags.answered, "\\Answered"),
                (m.flags.draft, "\\Draft"),
                (m.deleted, "\\Deleted"),
            ] {
                if on {
                    atoms.push(name.to_owned());
                }
            }
            (m.uid, MessageFlags::from_imap(atoms))
        })
        .collect();
    let state = ResyncState {
        uid_validity: status.uid_validity,
        highest_modseq: status.highest_modseq,
    };
    (local, state)
}

/// Another client flags 1, deletes 2 and adds a new message.
fn change_on_server(server: &MockImapServer) {
    let store = server.store();
    store.add_flags("INBOX", 1, &["\\Flagged", "$Work"]);
    store.remove_message("INBOX", 2);
    store.append("INBOX", &message("new"), NO_FLAGS);
}

fn assert_changes(result: &FlagResync) {
    assert!(!result.uid_validity_changed);
    let changed: Vec<u32> = result.changed.iter().map(|u| u.uid).collect();
    assert_eq!(changed, [1], "only the known, changed message");
    let flags = &result.changed[0].flags;
    assert!(flags.flags.seen && flags.flags.flagged);
    assert_eq!(flags.keywords, ["$Work"]);
    assert_eq!(result.vanished, [2]);
}

async fn run_scenario(capabilities: MockCapabilities, method: ResyncMethod) {
    let server = server_with(capabilities).await;
    let mut client = login(&server).await;
    let (local, state) = snapshot(&mut client).await;

    // Nothing changed yet.
    let result = client.resync_flags("INBOX", state, &local).await.unwrap();
    assert_eq!(result.method, method);
    assert!(result.changed.is_empty() && result.vanished.is_empty());

    change_on_server(&server);
    let result = client.resync_flags("INBOX", state, &local).await.unwrap();
    assert_eq!(result.method, method, "{result:?}");
    assert_changes(&result);
    let expected_modseq =
        (method != ResyncMethod::FullScan).then(|| server.store().highest_modseq("INBOX").unwrap());
    assert_eq!(result.next_state.highest_modseq, expected_modseq);

    // From the new state there is nothing more to report.
    let mut local = local;
    local.remove(&2);
    local.insert(1, result.changed[0].flags.clone());
    let again = client
        .resync_flags("INBOX", result.next_state, &local)
        .await
        .unwrap();
    assert!(
        again.changed.is_empty() && again.vanished.is_empty(),
        "{again:?}"
    );
}

#[tokio::test]
async fn qresync_finds_flag_changes_and_vanished_messages() {
    run_scenario(MockCapabilities::default(), ResyncMethod::Qresync).await;
}

#[tokio::test]
async fn condstore_without_qresync_gives_the_same_answer() {
    run_scenario(
        MockCapabilities {
            qresync: false,
            ..MockCapabilities::default()
        },
        ResyncMethod::Condstore,
    )
    .await;
}

#[tokio::test]
async fn servers_without_modseq_get_a_full_flag_diff() {
    run_scenario(MockCapabilities::imap4rev1(), ResyncMethod::FullScan).await;
}

#[tokio::test]
async fn qresync_uses_the_right_commands() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let (local, state) = snapshot(&mut client).await;
    change_on_server(&server);
    server.clear_commands();
    client.resync_flags("INBOX", state, &local).await.unwrap();
    // One EXAMINE with QRESYNC parameters answers everything.
    assert_eq!(server.commands(), ["EXAMINE"]);

    let server = server_with(MockCapabilities {
        qresync: false,
        ..MockCapabilities::default()
    })
    .await;
    let mut client = login(&server).await;
    let (local, state) = snapshot(&mut client).await;
    change_on_server(&server);
    server.clear_commands();
    client.resync_flags("INBOX", state, &local).await.unwrap();
    assert_eq!(server.commands(), ["EXAMINE", "UID FETCH", "UID SEARCH"]);
}

#[tokio::test]
async fn an_unknown_modseq_forces_a_full_scan_that_returns_a_modseq() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let (local, state) = snapshot(&mut client).await;
    change_on_server(&server);
    let state = ResyncState {
        highest_modseq: None,
        ..state
    };
    let result = client.resync_flags("INBOX", state, &local).await.unwrap();
    assert_eq!(result.method, ResyncMethod::FullScan);
    assert_changes(&result);
    assert_eq!(
        result.next_state.highest_modseq,
        server.store().highest_modseq("INBOX")
    );
}

#[tokio::test]
async fn a_changed_uidvalidity_is_reported_in_every_mode() {
    for capabilities in [
        MockCapabilities::default(),
        MockCapabilities {
            qresync: false,
            ..MockCapabilities::default()
        },
        MockCapabilities::imap4rev1(),
    ] {
        let server = server_with(capabilities).await;
        let mut client = login(&server).await;
        let (local, state) = snapshot(&mut client).await;
        let new_validity = server.store().reset_uid_validity("INBOX").unwrap();
        let result = client.resync_flags("INBOX", state, &local).await.unwrap();
        assert!(result.uid_validity_changed);
        assert!(result.changed.is_empty() && result.vanished.is_empty());
        assert_eq!(result.next_state.uid_validity, new_validity);
    }
}

#[tokio::test]
async fn an_emptied_mailbox_vanishes_everything() {
    for capabilities in [
        MockCapabilities::default(),
        MockCapabilities {
            qresync: false,
            ..MockCapabilities::default()
        },
        MockCapabilities::imap4rev1(),
    ] {
        let server = server_with(capabilities).await;
        let mut client = login(&server).await;
        let (local, state) = snapshot(&mut client).await;
        server.store().expunge_uids("INBOX", &[1, 2, 3]);
        let result = client.resync_flags("INBOX", state, &local).await.unwrap();
        assert_eq!(result.vanished, [1, 2, 3], "{capabilities:?}");
        assert_eq!(result.status.exists, 0);
    }
}
