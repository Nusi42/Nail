//! M2c carry-over (WP65): snoozes survive a restart of the [`Service`].
//!
//! `m2c_wired.rs` covers a wake time that passed while Nail was not
//! running. These tests cover snoozes that are *not yet due* when the
//! service stops: after reopening the same data directory they are still
//! listed with the same wake time and folder, the mail stays hidden, the
//! reloaded scheduler wakes each one on time (across a second restart,
//! too), and an unsnooze after a restart returns the mail at once.

#![cfg(test)]

use std::future::Future;
use std::time::Duration;

use nail_api::triage::SnoozedMessage;
use nail_api::{
    AccountId, Folder, FolderId, FolderRole, MessageId, MessageSummary, Request, Response,
};
use nail_core::demo::{DemoMulti, MULTI_WORK_ID, start_demo_multi};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(20);

struct Fixture {
    demo: DemoMulti,
    config_dir: TempDir,
    data_dir: TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let demo = start_demo_multi().await.unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        demo.write_config(config_dir.path()).unwrap();
        Self {
            demo,
            config_dir,
            data_dir,
        }
    }

    /// Opens a service on the fixture's (persistent) data directory.
    fn open(&self) -> Service {
        let options = self.demo.service_options(ServiceOptions::new(
            self.data_dir.path(),
            self.config_dir.path(),
        ));
        Service::open(options).unwrap()
    }

    fn now(&self) -> i64 {
        nail_triage::Clock::unix(&*self.demo.clock)
    }

    /// Subjects of the messages in a server mailbox and whether each one
    /// carries `$Snoozed`.
    fn server_mailbox(&self, mailbox: &str) -> Vec<(String, bool)> {
        let Ok(Value::Array(messages)) = self
            .demo
            .control("imap-messages", &json!({ "mailbox": mailbox }))
        else {
            return Vec::new();
        };
        messages
            .iter()
            .map(|m| {
                let snoozed = m["flags"]
                    .as_array()
                    .is_some_and(|f| f.iter().any(|f| f == "$Snoozed"));
                (
                    m["subject"].as_str().unwrap_or_default().to_owned(),
                    snoozed,
                )
            })
            .collect()
    }
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn folders(service: &Service) -> Vec<Folder> {
    match service
        .handle(Request::ListFolders {
            account_id: AccountId::new(MULTI_WORK_ID),
        })
        .await
    {
        Response::Folders { folders } => folders,
        other => panic!("unexpected {other:?}"),
    }
}

async fn folder(service: &Service, role: FolderRole) -> Option<Folder> {
    folders(service).await.into_iter().find(|f| f.role == role)
}

async fn messages(service: &Service, folder: &FolderId) -> Vec<MessageSummary> {
    match service
        .handle(Request::ListMessages {
            folder_id: folder.clone(),
            offset: 0,
            limit: 500,
        })
        .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

async fn contains(service: &Service, folder: &FolderId, id: &MessageId) -> bool {
    messages(service, folder).await.iter().any(|m| &m.id == id)
}

async fn snoozed(service: &Service) -> Vec<SnoozedMessage> {
    match service
        .handle(Request::ListSnoozed { account_id: None })
        .await
    {
        Response::Snoozed { messages } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

/// `(message id, wake time, wake folder)` of every snooze, soonest first.
async fn snooze_records(service: &Service) -> Vec<(MessageId, i64, FolderId)> {
    snoozed(service)
        .await
        .into_iter()
        .map(|s| {
            let until = chrono::DateTime::parse_from_rfc3339(&s.until)
                .unwrap()
                .timestamp();
            (s.message.id, until, s.wake_folder_id)
        })
        .collect()
}

async fn snooze(service: &Service, id: &MessageId, until: i64, wake: Option<FolderId>) {
    let until = chrono::DateTime::from_timestamp(until, 0)
        .unwrap()
        .to_rfc3339();
    let answer = service
        .handle(Request::SnoozeMessages {
            message_ids: vec![id.clone()],
            until,
            wake_folder_id: wake,
        })
        .await;
    assert!(matches!(answer, Response::Queued { .. }), "{answer:?}");
}

async fn synced_inbox(service: &Service) -> Folder {
    eventually("the work account synced", || async {
        match folder(service, FolderRole::Inbox).await {
            Some(inbox) => messages(service, &inbox.id).await.len() >= 15,
            None => false,
        }
    })
    .await;
    folder(service, FolderRole::Inbox).await.unwrap()
}

/// Nail's hidden folder never shows up in the folder list.
async fn assert_hidden_folder_stays_hidden(service: &Service) {
    let listed = folders(service).await;
    assert!(
        !listed.iter().any(|f| f.path == "Snoozed"),
        "the hidden Snoozed folder is not listed: {:?}",
        listed.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn pending_snoozes_survive_restarts_and_wake_on_time() {
    let f = Fixture::new().await;
    let service = f.open();
    let inbox = synced_inbox(&service).await;
    let archive = folder(&service, FolderRole::Archive)
        .await
        .expect("demo-multi work has an Archive folder");
    let mut inbox_mail = messages(&service, &inbox.id).await;
    let first = inbox_mail.remove(0);
    let second = inbox_mail.remove(0);
    let now = f.now();
    // The first comes back to the Inbox in an hour, the second to the
    // Archive (an explicit wake folder) in two hours.
    snooze(&service, &first.id, now + 3600, None).await;
    snooze(&service, &second.id, now + 7200, Some(archive.id.clone())).await;
    let before = snooze_records(&service).await;
    assert_eq!(
        before,
        vec![
            (first.id.clone(), now + 3600, inbox.id.clone()),
            (second.id.clone(), now + 7200, archive.id.clone()),
        ]
    );
    // Let the queue write both moves to the server before stopping.
    eventually("both are in the server's Snoozed folder", || async {
        let hidden = f.server_mailbox("Snoozed");
        [&first.subject, &second.subject]
            .iter()
            .all(|s| hidden.iter().any(|(subject, flag)| subject == *s && *flag))
    })
    .await;
    service.shutdown().await;
    drop(service);

    // Restart before any wake time: nothing is due yet.
    let service = f.open();
    assert_eq!(snooze_records(&service).await, before, "the same snoozes");
    assert!(
        !contains(&service, &inbox.id, &first.id).await,
        "still hidden"
    );
    assert!(!contains(&service, &archive.id, &second.id).await);
    assert_hidden_folder_stays_hidden(&service).await;
    // Give the reloaded wake task a few polls: nothing may wake early.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(snooze_records(&service).await, before);
    assert!(!contains(&service, &inbox.id, &first.id).await);

    // The clock passes the first wake time: only the first returns, woken
    // by the reloaded scheduler (no request touches the snoozes).
    f.demo.clock.advance(3601);
    eventually("the first is back in the Inbox", || async {
        contains(&service, &inbox.id, &first.id).await
    })
    .await;
    assert_eq!(
        snooze_records(&service).await,
        vec![(second.id.clone(), now + 7200, archive.id.clone())]
    );
    assert!(!contains(&service, &archive.id, &second.id).await);
    eventually(
        "the server's Inbox has the first without $Snoozed",
        || async {
            f.server_mailbox("INBOX")
                .iter()
                .any(|(subject, flag)| subject == &first.subject && !flag)
        },
    )
    .await;
    service.shutdown().await;
    drop(service);

    // A second restart keeps the remaining snooze.
    let service = f.open();
    assert_eq!(
        snooze_records(&service).await,
        vec![(second.id.clone(), now + 7200, archive.id.clone())]
    );
    assert!(contains(&service, &inbox.id, &first.id).await);
    assert_hidden_folder_stays_hidden(&service).await;
    f.demo.clock.advance(3600);
    eventually("the second is back in the Archive", || async {
        contains(&service, &archive.id, &second.id).await
    })
    .await;
    assert!(snoozed(&service).await.is_empty());
    assert!(
        !contains(&service, &inbox.id, &second.id).await,
        "its own wake folder"
    );
    eventually("the server's Snoozed folder is empty", || async {
        f.server_mailbox("Snoozed").is_empty()
    })
    .await;
    service.shutdown().await;
}

#[tokio::test]
async fn unsnoozing_after_a_restart_returns_the_mail_at_once() {
    let f = Fixture::new().await;
    let service = f.open();
    let inbox = synced_inbox(&service).await;
    let message = messages(&service, &inbox.id).await.remove(0);
    snooze(&service, &message.id, f.now() + 86_400, None).await;
    eventually("it is in the server's Snoozed folder", || async {
        f.server_mailbox("Snoozed")
            .iter()
            .any(|(subject, flag)| subject == &message.subject && *flag)
    })
    .await;
    service.shutdown().await;
    drop(service);

    let service = f.open();
    assert_eq!(snoozed(&service).await.len(), 1, "the snooze survived");
    assert!(!contains(&service, &inbox.id, &message.id).await);
    let answer = service
        .handle(Request::UnsnoozeMessages {
            message_ids: vec![message.id.clone()],
        })
        .await;
    assert!(matches!(answer, Response::Queued { .. }), "{answer:?}");
    assert!(snoozed(&service).await.is_empty());
    assert!(
        contains(&service, &inbox.id, &message.id).await,
        "back in the Inbox at once"
    );
    eventually("the server's Inbox has it without $Snoozed", || async {
        f.server_mailbox("INBOX")
            .iter()
            .any(|(subject, flag)| subject == &message.subject && !flag)
    })
    .await;
    service.shutdown().await;
    drop(service);

    // Nothing comes back after another restart, even past the old wake time.
    let service = f.open();
    f.demo.clock.advance(2 * 86_400);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(snoozed(&service).await.is_empty());
    let copies = messages(&service, &inbox.id)
        .await
        .iter()
        .filter(|m| m.id == message.id)
        .count();
    assert_eq!(copies, 1, "exactly one copy in the Inbox");
    service.shutdown().await;
}
