//! M2c integration (ADR 0048) against the `demo-multi` mock servers:
//! Sieve uploads and rule placement, vacation, junk training and
//! classification, snooze with the adjustable clock, new-mail
//! notifications and quiet hours, headers-only sync with on-demand bodies,
//! and the server-side search fallback.

use std::future::Future;
use std::time::Duration;

use nail_api::rules::{Condition, FolderRef, Rule, RuleAction, RuleTrigger, TextMatch, TextOp};
use nail_api::sieve::{RulePlacement, SieveState, VacationSettings};
use nail_api::triage::{
    JunkSettings, JunkSource, NotificationKind, NotificationSettings, QuietHours,
};
use nail_api::{
    AccountId, Event, Folder, FolderId, FolderRole, MessageId, MessageSummary, Request, Response,
    SearchQuery,
};
use nail_core::demo::{DemoMulti, MULTI_HEADERS_ONLY_FOLDER, MULTI_WORK_ID, start_demo_multi};
use nail_core::{Service, ServiceOptions};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::broadcast;

const TIMEOUT: Duration = Duration::from_secs(20);

struct Fixture {
    service: Service,
    demo: DemoMulti,
    _config_dir: TempDir,
    _data_dir: TempDir,
}

async fn setup() -> Fixture {
    let demo = start_demo_multi().await.unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    demo.write_config(config_dir.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    let service = Service::open(options).unwrap();
    Fixture {
        service,
        demo,
        _config_dir: config_dir,
        _data_dir: data_dir,
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

async fn in_folder(service: &Service, role: FolderRole, subject: &str) -> Option<MessageSummary> {
    let folder = folder(service, role).await?;
    messages(service, &folder.id)
        .await
        .into_iter()
        .find(|m| m.subject.contains(subject))
}

async fn synced(f: &Fixture) {
    eventually("the work account synced", || async {
        match folder(&f.service, FolderRole::Inbox).await {
            Some(inbox) => messages(&f.service, &inbox.id).await.len() >= 15,
            None => false,
        }
    })
    .await;
}

fn deliver(f: &Fixture, subject: &str, body: &str) {
    let raw = format!(
        "From: Someone <someone@example.net>\r\nTo: ada@work.example\r\nSubject: {subject}\r\n\
         Date: Wed, 30 Sep 2026 09:00:00 +0000\r\nMessage-ID: <{}@example.net>\r\n\r\n{body}\r\n",
        subject.replace(' ', "-")
    );
    f.demo
        .control("imap-deliver", &json!({ "raw": raw }))
        .unwrap();
}

async fn sieve_status(service: &Service) -> nail_api::sieve::SieveAccountStatus {
    match service
        .handle(Request::GetSieveStatus {
            account_id: Some(MULTI_WORK_ID.into()),
        })
        .await
    {
        Response::SieveStatus { mut accounts } => accounts.remove(0),
        other => panic!("unexpected {other:?}"),
    }
}

fn subject_rule(name: &str, subject: &str) -> Rule {
    Rule {
        id: "".into(),
        name: name.into(),
        enabled: true,
        triggers: vec![RuleTrigger::Arrival, RuleTrigger::Manual],
        scope: Vec::new(),
        conditions: Condition::All {
            conditions: vec![Condition::Subject {
                matches: TextMatch {
                    op: TextOp::Contains,
                    value: subject.into(),
                    case_sensitive: false,
                },
            }],
        },
        actions: vec![
            RuleAction::Move {
                folder: FolderRef::Role {
                    role: FolderRole::Archive,
                    account_id: None,
                },
            },
            RuleAction::Flag { flagged: true },
        ],
        source: None,
    }
}

async fn next_notification(
    events: &mut broadcast::Receiver<Event>,
    within: Duration,
) -> Option<nail_api::triage::MailNotification> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, events.recv()).await {
            Ok(Ok(Event::NotificationRequested { notification })) => return Some(notification),
            Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => return None,
        }
    }
}

/// Turns the demo account's ManageSieve on or off (demo-multi starts with
/// it off: the mock server stores scripts but does not run them).
async fn set_sieve(service: &Service, enabled: bool) {
    let Response::AccountsConfig { config } = service.handle(Request::GetAccountsConfig).await
    else {
        panic!("no config")
    };
    let mut work = config.account(&MULTI_WORK_ID.into()).unwrap().clone();
    work.sieve.as_mut().unwrap().enabled = Some(enabled);
    assert!(matches!(
        service
            .handle(Request::UpdateAccount {
                config: Box::new(work),
                secrets: vec![],
            })
            .await,
        Response::AccountSaved { .. }
    ));
}

#[tokio::test]
async fn rules_and_vacation_run_on_the_sieve_server() {
    let f = setup().await;
    synced(&f).await;
    assert_eq!(sieve_status(&f.service).await.state, SieveState::Disabled);
    set_sieve(&f.service, true).await;
    eventually("Nail's script is active on the server", || async {
        f.demo.sieve.active().as_deref() == Some("nail")
    })
    .await;
    eventually("in sync", || async {
        sieve_status(&f.service).await.state == SieveState::InSync
    })
    .await;

    let Response::RuleSaved { rule } = f
        .service
        .handle(Request::SaveRule {
            rule: Box::new(subject_rule("Invoices", "old invoice")),
            position: None,
        })
        .await
    else {
        panic!("rule not saved")
    };
    eventually("the rule runs on the server", || async {
        let status = sieve_status(&f.service).await;
        status.state == SieveState::InSync
            && status
                .rules
                .iter()
                .any(|r| r.rule_id == rule.id && r.placement == RulePlacement::Server)
    })
    .await;
    let script = f.demo.sieve.script("nail").unwrap();
    assert!(script.contains("old invoice"), "{script}");
    assert!(script.contains("fileinto"), "{script}");

    // The mock server does not run scripts: the message stays in the Inbox
    // because the client leaves server rules alone.
    deliver(&f, "An old invoice", "Please pay.");
    eventually("the message arrived", || async {
        in_folder(&f.service, FolderRole::Inbox, "An old invoice")
            .await
            .is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let arrived = in_folder(&f.service, FolderRole::Inbox, "An old invoice")
        .await
        .expect("still in the Inbox");
    assert!(!arrived.flags.flagged, "the client did not run the rule");

    // Vacation reply.
    let away = VacationSettings {
        enabled: true,
        subject: "Away".into(),
        body: "I am away until Monday.".into(),
        ..VacationSettings::default()
    };
    assert!(matches!(
        f.service
            .handle(Request::UpdateVacation {
                account_id: MULTI_WORK_ID.into(),
                settings: away,
            })
            .await,
        Response::Vacation { .. }
    ));
    eventually("the vacation reply is uploaded", || async {
        f.demo
            .sieve
            .script("nail")
            .is_some_and(|s| s.contains("vacation"))
            && sieve_status(&f.service).await.vacation_active
    })
    .await;
    let Response::SieveScript { script, rules, .. } = f
        .service
        .handle(Request::PreviewSieveScript {
            account_id: MULTI_WORK_ID.into(),
        })
        .await
    else {
        panic!("no preview")
    };
    assert!(script.contains("I am away until Monday."), "{script}");
    assert!(rules.iter().any(|r| r.rule_id == rule.id));

    // A forced upload answers `uploading` and ends in sync.
    let uploads = f.demo.sieve.uploads().len();
    let Response::SieveStatus { accounts } = f
        .service
        .handle(Request::SyncSieve {
            account_id: MULTI_WORK_ID.into(),
        })
        .await
    else {
        panic!("no status")
    };
    assert_eq!(accounts[0].state, SieveState::Uploading);
    eventually("uploaded again", || async {
        f.demo.sieve.uploads().len() > uploads
            && sieve_status(&f.service).await.state == SieveState::InSync
    })
    .await;

    // Switching ManageSieve off removes Nail's script.
    set_sieve(&f.service, false).await;
    eventually("the script is removed", || async {
        f.demo.sieve.script("nail").is_none()
    })
    .await;
    let status = sieve_status(&f.service).await;
    assert_eq!(status.state, SieveState::Disabled);
    assert!(
        status
            .rules
            .iter()
            .all(|r| r.placement != RulePlacement::Server)
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn marking_junk_trains_and_similar_mail_lands_in_junk() {
    let f = setup().await;
    synced(&f).await;
    let spam = "Congratulations winner! Claim your lottery prize money now, cheap pills, \
                exclusive casino bonus, click here to claim the prize today";
    deliver(&f, "You are a lottery winner", spam);
    deliver(
        &f,
        "Minutes of the design review",
        "Notes from the design review meeting about the parser and the release plan",
    );
    eventually("both arrived", || async {
        in_folder(&f.service, FolderRole::Inbox, "lottery winner")
            .await
            .is_some()
            && in_folder(&f.service, FolderRole::Inbox, "design review")
                .await
                .is_some()
    })
    .await;
    let settings = JunkSettings {
        enabled: true,
        min_training: 1,
        threshold: 60,
        ..JunkSettings::default()
    };
    assert!(matches!(
        f.service
            .handle(Request::UpdateJunkSettings {
                account_id: MULTI_WORK_ID.into(),
                settings,
            })
            .await,
        Response::JunkStatus { .. }
    ));
    let spam_id = in_folder(&f.service, FolderRole::Inbox, "lottery winner")
        .await
        .unwrap()
        .id;
    let ham_id = in_folder(&f.service, FolderRole::Inbox, "design review")
        .await
        .unwrap()
        .id;
    for (id, junk) in [(spam_id.clone(), true), (ham_id, false)] {
        assert!(matches!(
            f.service
                .handle(Request::MarkJunk {
                    message_ids: vec![id],
                    junk,
                })
                .await,
            Response::Queued { .. }
        ));
    }
    // The junk mail moved to Junk (locally at once, on the server by replay).
    assert!(
        in_folder(&f.service, FolderRole::Junk, "lottery winner")
            .await
            .is_some()
    );
    let Response::JunkStatus { status } = f
        .service
        .handle(Request::GetJunkStatus {
            account_id: MULTI_WORK_ID.into(),
        })
        .await
    else {
        panic!("no status")
    };
    assert_eq!((status.trained_junk, status.trained_ham), (1, 1));
    assert!(status.ready);
    let Response::Message { message } = f.service.handle(Request::GetMessage { id: spam_id }).await
    else {
        panic!("no message")
    };
    assert_eq!(
        message.junk.map(|v| (v.junk, v.source)),
        Some((true, JunkSource::User))
    );

    // Similar new mail is classified and filed as junk.
    deliver(&f, "Another lottery winner", spam);
    eventually("the classifier files it in Junk", || async {
        in_folder(&f.service, FolderRole::Junk, "Another lottery winner")
            .await
            .is_some()
    })
    .await;
    eventually("the server moved it too", || async {
        let junk = f
            .demo
            .control("imap-messages", &json!({ "mailbox": "Junk" }))
            .unwrap();
        junk.as_array()
            .unwrap()
            .iter()
            .any(|m| m["subject"] == "Another lottery winner")
    })
    .await;
    f.service.shutdown().await;
}

#[tokio::test]
async fn snoozed_mail_hides_and_returns_at_the_wake_time() {
    let f = setup().await;
    synced(&f).await;
    let inbox = folder(&f.service, FolderRole::Inbox).await.unwrap();
    let message = messages(&f.service, &inbox.id).await.remove(0);
    let now = nail_triage::Clock::unix(&*f.demo.clock);
    let until = chrono::DateTime::from_timestamp(now + 3600, 0)
        .unwrap()
        .to_rfc3339();
    let mut events = f.service.subscribe();
    assert!(matches!(
        f.service
            .handle(Request::SnoozeMessages {
                message_ids: vec![message.id.clone()],
                until: until.clone(),
                wake_folder_id: None,
            })
            .await,
        Response::Queued { .. }
    ));
    assert!(
        !messages(&f.service, &inbox.id)
            .await
            .iter()
            .any(|m| m.id == message.id),
        "hidden at once"
    );
    assert!(
        !folders(&f.service)
            .await
            .iter()
            .any(|f| f.path == "Snoozed"),
        "the Snoozed folder is hidden"
    );
    let Response::Snoozed { messages: snoozed } = f
        .service
        .handle(Request::ListSnoozed { account_id: None })
        .await
    else {
        panic!("no list")
    };
    assert_eq!(snoozed.len(), 1);
    assert_eq!(snoozed[0].wake_folder_id, inbox.id);
    eventually("the server has it in Snoozed with $Snoozed", || async {
        f.demo
            .control("imap-messages", &json!({ "mailbox": "Snoozed" }))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .is_some_and(|m| {
                m.iter().any(|m| {
                    m["flags"]
                        .as_array()
                        .is_some_and(|f| f.iter().any(|f| f == "$Snoozed"))
                })
            })
    })
    .await;
    // Too early: nothing happens.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !messages(&f.service, &inbox.id)
            .await
            .iter()
            .any(|m| m.id == message.id)
    );
    // The clock passes the wake time.
    f.demo.clock.advance(3601);
    eventually("back in the Inbox", || async {
        messages(&f.service, &inbox.id)
            .await
            .iter()
            .any(|m| m.id == message.id)
    })
    .await;
    let woke = next_notification(&mut events, TIMEOUT).await.unwrap();
    assert_eq!(woke.kind, NotificationKind::SnoozeWoke);
    assert_eq!(woke.message_ids, vec![message.id.clone()]);
    let Response::Snoozed { messages: snoozed } = f
        .service
        .handle(Request::ListSnoozed { account_id: None })
        .await
    else {
        panic!("no list")
    };
    assert!(snoozed.is_empty());
    // A wake time in the past is refused with its text.
    let past = chrono::DateTime::from_timestamp(now - 60, 0)
        .unwrap()
        .to_rfc3339();
    let Response::Error { localized, .. } = f
        .service
        .handle(Request::SnoozeMessages {
            message_ids: vec![message.id.clone()],
            until: past,
            wake_folder_id: None,
        })
        .await
    else {
        panic!("a past time is refused")
    };
    assert_eq!(
        localized.map(|l| l.id).as_deref(),
        Some("snooze-error-not-in-future")
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn new_mail_notifies_except_in_quiet_hours() {
    let f = setup().await;
    synced(&f).await;
    // Local noon today, so a quiet-hours range around it is simple.
    let noon = chrono::Local::now()
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .earliest()
        .unwrap()
        .timestamp();
    f.demo.clock.set(noon);
    let mut events = f.service.subscribe();
    deliver(&f, "Lunch on Friday?", "Shall we?");
    let notification = next_notification(&mut events, TIMEOUT)
        .await
        .expect("a new-mail notification");
    assert_eq!(notification.kind, NotificationKind::NewMail);
    assert_eq!(notification.title.id, "notification-new-mail-title");
    assert_eq!(notification.message_ids.len(), 1);

    let quiet = NotificationSettings {
        quiet_hours: Some(QuietHours {
            start: "11:00".into(),
            end: "13:00".into(),
            days: Vec::new(),
        }),
        ..NotificationSettings::default()
    };
    assert!(matches!(
        f.service
            .handle(Request::UpdateNotificationSettings { settings: quiet })
            .await,
        Response::NotificationSettings { .. }
    ));
    deliver(&f, "Quiet please", "Shh.");
    eventually("the quiet mail arrived", || async {
        in_folder(&f.service, FolderRole::Inbox, "Quiet please")
            .await
            .is_some()
    })
    .await;
    assert!(
        next_notification(&mut events, Duration::from_secs(4))
            .await
            .is_none(),
        "quiet hours suppress it"
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn headers_only_mail_is_found_on_the_server_and_fetched_on_open() {
    let f = setup().await;
    synced(&f).await;
    eventually("the headers-only folder is synced", || async {
        match folders(&f.service)
            .await
            .into_iter()
            .find(|f| f.path == MULTI_HEADERS_ONLY_FOLDER)
        {
            Some(folder) => !messages(&f.service, &folder.id).await.is_empty(),
            None => false,
        }
    })
    .await;
    let newsletters = folders(&f.service)
        .await
        .into_iter()
        .find(|f| f.path == MULTI_HEADERS_ONLY_FOLDER)
        .unwrap();
    let listed = messages(&f.service, &newsletters.id).await.remove(0);
    assert!(listed.headers_only, "synced without its body");

    // The word is only in the body: the server search finds it.
    let Response::SearchResults { hits, server, .. } = f
        .service
        .handle(Request::Search {
            query: SearchQuery {
                text: "platypus".into(),
                account_id: None,
                folder_id: None,
                offset: 0,
                limit: 50,
                sort: Default::default(),
            },
        })
        .await
    else {
        panic!("no results")
    };
    assert!(
        hits.iter().any(|h| h.message.id == listed.id),
        "{hits:?} {server:?}"
    );
    let server = server.expect("the server was asked");
    assert_eq!(server.accounts, vec![AccountId::new(MULTI_WORK_ID)]);
    assert_eq!(server.hits, 1);
    assert!(server.failed.is_empty());

    // Opening it fetches the body.
    let Response::Message { message } = f
        .service
        .handle(Request::GetMessage {
            id: listed.id.clone(),
        })
        .await
    else {
        panic!("no message")
    };
    assert!(message.text.contains("platypus"), "{}", message.text);
    let listed: Vec<MessageId> = messages(&f.service, &newsletters.id)
        .await
        .into_iter()
        .filter(|m| m.headers_only)
        .map(|m| m.id)
        .collect();
    assert!(listed.is_empty(), "the body is stored now");
    f.service.shutdown().await;
}
