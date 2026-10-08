//! M2c integration (ADR 0048) against the `demo-multi` mock servers:
//! Sieve uploads and rule placement, vacation, junk training and
//! classification, snooze with the adjustable clock, new-mail
//! notifications and quiet hours, headers-only sync with on-demand bodies,
//! and the server-side search fallback.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{
    Multi, TIMEOUT, archive_and_flag, eventually, folder, folders, messages, save_rule,
    update_account,
};
use nail_api::Folder;

use std::time::Duration;

use nail_api::rules::{Rule, RuleTrigger};
use nail_api::sieve::{RulePlacement, SieveState, VacationSettings};
use nail_api::triage::{
    JunkSettings, JunkSource, NotificationKind, NotificationSettings, QuietHours,
};
use nail_api::{
    AccountId, Event, FolderId, FolderRole, MessageId, MessageSummary, Request, Response,
    SearchQuery,
};
use nail_core::Service;
use nail_core::demo::{MULTI_HEADERS_ONLY_FOLDER, MULTI_WORK_ID, start_demo_multi};
use serde_json::json;
use tokio::sync::broadcast;

type Fixture = Multi;

async fn setup() -> Fixture {
    Multi::start().await
}

async fn in_folder(service: &Service, role: FolderRole, subject: &str) -> Option<MessageSummary> {
    let folder = folder(service, MULTI_WORK_ID, role).await?;
    messages(service, &folder.id)
        .await
        .into_iter()
        .find(|m| m.subject.contains(subject))
}

async fn synced(f: &Fixture) {
    eventually("the work account synced", || async {
        match folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox).await {
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
    common::subject_rule(
        name,
        vec![RuleTrigger::Arrival, RuleTrigger::Manual],
        subject,
        archive_and_flag(),
    )
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
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => return None,
        }
    }
}

/// Turns the demo account's ManageSieve on or off (demo-multi starts with
/// it off: the mock server stores scripts but does not run them).
async fn set_sieve(service: &Service, enabled: bool) {
    update_account(service, MULTI_WORK_ID, |work| {
        work.sieve.as_mut().unwrap().enabled = Some(enabled);
    })
    .await;
}

/// Saves the rule "Invoices" and waits until it runs on the server.
async fn invoices_on_the_server(f: &Fixture) -> Rule {
    let rule = save_rule(&f.service, subject_rule("Invoices", "old invoice")).await;
    eventually("the rule runs on the server", || async {
        let status = sieve_status(&f.service).await;
        status.state == SieveState::InSync
            && status
                .rules
                .iter()
                .any(|r| r.rule_id == rule.id && r.placement == RulePlacement::Server)
    })
    .await;
    rule
}

/// The work account's inbox.
async fn work_inbox(f: &Fixture) -> Folder {
    folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox)
        .await
        .unwrap()
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

    let rule = invoices_on_the_server(&f).await;
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

async fn set_vacation(service: &Service, body: Option<&str>) {
    let settings = VacationSettings {
        enabled: body.is_some(),
        subject: "Away".into(),
        body: body.unwrap_or("Back soon.").into(),
        ..VacationSettings::default()
    };
    assert!(matches!(
        service
            .handle(Request::UpdateVacation {
                account_id: MULTI_WORK_ID.into(),
                settings,
            })
            .await,
        Response::Vacation { .. }
    ));
}

/// Review fixes (ADR 0049): server rules stay skipped while an upload
/// fails (the old script is still active), a renamed script leaves its
/// old place, and switching off / deleting the account removes the script
/// under its recorded name.
#[tokio::test]
async fn the_server_script_is_tracked_across_failures_renames_and_deletion() {
    let f = setup().await;
    synced(&f).await;
    set_sieve(&f.service, true).await;
    invoices_on_the_server(&f).await;

    // Uploads fail from now on; the uploaded script stays active.
    f.demo
        .sieve
        .reject_scripts_containing("Gone fishing", "vacation replies are disabled");
    set_vacation(&f.service, Some("Gone fishing.")).await;
    eventually("the upload failed", || async {
        sieve_status(&f.service).await.state == SieveState::Failed
    })
    .await;
    deliver(&f, "Another old invoice", "Please pay.");
    eventually("the message arrived", || async {
        in_folder(&f.service, FolderRole::Inbox, "Another old invoice")
            .await
            .is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let arrived = in_folder(&f.service, FolderRole::Inbox, "Another old invoice")
        .await
        .expect("still in the Inbox: the server runs the rule");
    assert!(!arrived.flags.flagged, "the client did not run it too");
    set_vacation(&f.service, None).await;
    eventually("in sync again", || async {
        sieve_status(&f.service).await.state == SieveState::InSync
    })
    .await;

    // A new script name: the old script goes, the new one is active and
    // never includes the old one.
    update_account(&f.service, MULTI_WORK_ID, |w| {
        w.sieve.as_mut().unwrap().script_name = Some("nail-renamed".into());
    })
    .await;
    eventually("the renamed script is active", || async {
        f.demo.sieve.active().as_deref() == Some("nail-renamed")
            && f.demo.sieve.script("nail").is_none()
    })
    .await;
    let script = f.demo.sieve.script("nail-renamed").unwrap();
    assert!(!script.contains("\"nail\""), "{script}");

    // Removing the node switches off under the recorded name.
    update_account(&f.service, MULTI_WORK_ID, |w| w.sieve = None).await;
    eventually("the renamed script is removed", || async {
        f.demo.sieve.script("nail-renamed").is_none()
    })
    .await;
    assert_eq!(f.demo.sieve.active(), None);

    // Deleting the account removes its script too.
    update_account(&f.service, MULTI_WORK_ID, |w| {
        w.sieve = Some(nail_api::sieve::ManageSieveConfig {
            port: Some(f.demo.sieve.port()),
            host: Some("127.0.0.1".into()),
            tls: Some(nail_api::accounts::TlsMode::Plaintext),
            ..nail_api::sieve::ManageSieveConfig::default()
        });
    })
    .await;
    eventually("uploaded again", || async {
        f.demo.sieve.active().as_deref() == Some("nail")
    })
    .await;
    assert!(matches!(
        f.service
            .handle(Request::RemoveAccount {
                account_id: MULTI_WORK_ID.into(),
            })
            .await,
        Response::Done
    ));
    assert_eq!(f.demo.sieve.script("nail"), None);
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
    let inbox = work_inbox(&f).await;
    let message = messages(&f.service, &inbox.id).await.remove(0);
    let until = f.in_an_hour();
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
        !folders(&f.service, MULTI_WORK_ID)
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
    assert!(snoozed.is_empty(), "{snoozed:?}");
    // A wake time in the past is refused with its text.
    let past = chrono::DateTime::from_timestamp(nail_triage::Clock::unix(&*f.demo.clock) - 60, 0)
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

/// Review fixes (ADR 0049): snoozes survive a restart (missed wake times
/// are due at once), and each folder the mail returns to gets its own
/// notification, so none names mail of another account or folder.
#[tokio::test]
async fn snoozes_survive_a_restart_and_notify_per_folder() {
    let f = setup().await;
    synced(&f).await;
    let inbox = work_inbox(&f).await;
    let lists = folders(&f.service, MULTI_WORK_ID)
        .await
        .into_iter()
        .find(|x| x.path == MULTI_HEADERS_ONLY_FOLDER)
        .unwrap();
    let a = messages(&f.service, &inbox.id).await.remove(0);
    eventually("the other folder synced", || async {
        !messages(&f.service, &lists.id).await.is_empty()
    })
    .await;
    let b = messages(&f.service, &lists.id).await.remove(0);
    let until = f.in_an_hour();
    assert!(matches!(
        f.service
            .handle(Request::SnoozeMessages {
                message_ids: vec![a.id.clone(), b.id.clone()],
                until,
                wake_folder_id: None,
            })
            .await,
        Response::Queued { .. }
    ));
    f.service.shutdown().await;

    // Nail is not running when the wake time passes.
    f.demo.clock.advance(7200);
    let service = Service::open(f.options()).unwrap();
    let mut events = service.subscribe();
    eventually("both came back after the restart", || async {
        messages(&service, &inbox.id)
            .await
            .iter()
            .any(|m| m.id == a.id)
            && messages(&service, &lists.id)
                .await
                .iter()
                .any(|m| m.id == b.id)
    })
    .await;
    let mut woke = Vec::new();
    while woke.len() < 2 {
        let n = next_notification(&mut events, TIMEOUT)
            .await
            .expect("a notification per folder");
        if n.kind == NotificationKind::SnoozeWoke {
            woke.push(n);
        }
    }
    woke.sort_by_key(|n| n.folder_id.clone());
    let mut expected = vec![
        (Some(inbox.id.clone()), vec![a.id.clone()]),
        (Some(lists.id.clone()), vec![b.id.clone()]),
    ];
    expected.sort();
    let got: Vec<(Option<FolderId>, Vec<MessageId>)> = woke
        .iter()
        .map(|n| (n.folder_id.clone(), n.message_ids.clone()))
        .collect();
    assert_eq!(got, expected);
    assert!(
        woke.iter()
            .all(|n| n.account_id.as_ref().map(AccountId::as_str) == Some(MULTI_WORK_ID))
    );
    service.shutdown().await;
}

/// Review fixes (ADR 0049): a folder of the user called `Snoozed` stays
/// visible and is not used; Nail's own hidden folder gets another name;
/// mail stranded there without a snooze comes back to the Inbox after a
/// restart.
#[tokio::test]
async fn a_users_snoozed_folder_stays_visible_and_stranded_mail_returns() {
    let demo = start_demo_multi().await.unwrap();
    assert!(demo.work.store().add_mailbox("Snoozed", FolderRole::Other));
    demo.control(
        "imap-deliver",
        &json!({
            "mailbox": "Snoozed",
            "raw": "From: Me <ada@work.example>\r\nSubject: My own snoozed note\r\n\
                    Date: Wed, 30 Sep 2026 09:00:00 +0000\r\nMessage-ID: <note@work.example>\r\n\r\nmine\r\n",
        }),
    )
    .unwrap();
    let f = Multi::with(demo, |_, _| {});
    synced(&f).await;
    eventually(
        "the user's Snoozed folder is listed with its mail",
        || async {
            match folders(&f.service, MULTI_WORK_ID)
                .await
                .into_iter()
                .find(|x| x.path == "Snoozed")
            {
                Some(mine) => messages(&f.service, &mine.id)
                    .await
                    .iter()
                    .any(|m| m.subject == "My own snoozed note"),
                None => false,
            }
        },
    )
    .await;

    let inbox = work_inbox(&f).await;
    let message = messages(&f.service, &inbox.id).await.remove(0);
    let until = f.in_an_hour();
    assert!(matches!(
        f.service
            .handle(Request::SnoozeMessages {
                message_ids: vec![message.id.clone()],
                until,
                wake_folder_id: None,
            })
            .await,
        Response::Queued { .. }
    ));
    let listed = folders(&f.service, MULTI_WORK_ID).await;
    assert!(listed.iter().any(|x| x.path == "Snoozed"), "still visible");
    assert!(!listed.iter().any(|x| x.path == "Nail Snoozed"), "hidden");
    eventually(
        "the snoozed message is in Nail's own folder on the server",
        || async {
            f.demo
                .control("imap-messages", &json!({ "mailbox": "Nail Snoozed" }))
                .ok()
                .and_then(|v| v.as_array().map(Vec::len))
                == Some(1)
        },
    )
    .await;
    let mine = f
        .demo
        .control("imap-messages", &json!({ "mailbox": "Snoozed" }))
        .unwrap();
    assert_eq!(
        mine.as_array().unwrap().len(),
        1,
        "the user's folder is untouched"
    );

    // A message lands in Nail's hidden folder without a snooze (another
    // client, or a snooze lost to a crash of an older version).
    f.demo
        .control(
            "imap-deliver",
            &json!({
                "mailbox": "Nail Snoozed",
                "raw": "From: Someone <someone@example.net>\r\nSubject: Stranded mail\r\n\
                        Date: Wed, 30 Sep 2026 09:00:00 +0000\r\n\r\nlost\r\n",
            }),
        )
        .unwrap();
    let Response::Snoozed { messages: snoozed } = f
        .service
        .handle(Request::ListSnoozed { account_id: None })
        .await
    else {
        panic!("no list")
    };
    let hidden = snoozed[0].message.folder_id.clone();
    eventually(
        "the stranded mail synced into the hidden folder",
        || async {
            f.service
                .handle(Request::SyncNow {
                    account_id: Some(MULTI_WORK_ID.into()),
                })
                .await;
            messages(&f.service, &hidden).await.len() == 2
        },
    )
    .await;
    f.service.shutdown().await;

    let service = Service::open(f.options()).unwrap();
    eventually("the stranded mail is back in the Inbox", || async {
        messages(&service, &inbox.id)
            .await
            .iter()
            .any(|m| m.subject == "Stranded mail")
    })
    .await;
    let still: Vec<MessageId> = messages(&service, &hidden)
        .await
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(
        still,
        std::slice::from_ref(&message.id),
        "the real snooze stays"
    );
    service.shutdown().await;
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

async fn search_page(
    service: &Service,
    offset: u32,
    limit: u32,
) -> (
    u32,
    Vec<nail_api::SearchHit>,
    Option<nail_api::search::ServerSearch>,
) {
    match service
        .handle(Request::Search {
            query: SearchQuery {
                text: "platypus".into(),
                account_id: None,
                folder_id: None,
                offset,
                limit,
                sort: nail_api::search::SearchSort::default(),
            },
        })
        .await
    {
        Response::SearchResults {
            total,
            hits,
            server,
            ..
        } => (total, hits, server),
        other => panic!("unexpected {other:?}"),
    }
}

/// Review fix (ADR 0049): server hits come on top of the first page, are
/// marked, at most a page of them, and paging counts local hits only, so
/// the second page does not skip local hits.
#[tokio::test]
async fn server_search_hits_do_not_shift_the_pages() {
    let f = setup().await;
    synced(&f).await;
    for n in 0..3 {
        deliver(
            &f,
            &format!("Local platypus {n}"),
            "A platypus in the body.",
        );
    }
    for n in 0..2 {
        let raw = format!(
            "From: List <list@example.net>\r\nSubject: Letter {n}\r\n\
             Date: Wed, 30 Sep 2026 09:00:00 +0000\r\nMessage-ID: <letter{n}@example.net>\r\n\r\n\
             More platypus news.\r\n"
        );
        f.demo
            .control(
                "imap-deliver",
                &json!({ "mailbox": MULTI_HEADERS_ONLY_FOLDER, "raw": raw }),
            )
            .unwrap();
    }
    eventually("everything synced and indexed", || async {
        f.service
            .handle(Request::SyncNow {
                account_id: Some(MULTI_WORK_ID.into()),
            })
            .await;
        let (_, hits, _) = search_page(&f.service, 0, 50).await;
        hits.iter().filter(|h| !h.from_server).count() == 3
            && hits.iter().filter(|h| h.from_server).count() == 3
    })
    .await;

    let (total, first, server) = search_page(&f.service, 0, 2).await;
    let local: Vec<MessageId> = first
        .iter()
        .filter(|h| !h.from_server)
        .map(|h| h.message.id.clone())
        .collect();
    let from_server = first.iter().filter(|h| h.from_server).count();
    assert_eq!(local.len(), 2);
    assert_eq!(from_server, 2, "at most a page of server hits");
    assert_eq!(server.unwrap().hits, 2);
    assert_eq!(total, 3 + 2, "local total plus the server hits shown");
    assert!(
        first
            .iter()
            .filter(|h| h.from_server)
            .all(|h| h.message.headers_only)
    );

    // The next page starts after the local hits: the third local hit.
    let (total, second, server) = search_page(&f.service, 2, 2).await;
    assert!(
        server.is_none(),
        "the servers are asked on the first page only"
    );
    assert_eq!(total, 3);
    assert_eq!(second.len(), 1);
    assert!(!second[0].from_server);
    assert!(!local.contains(&second[0].message.id));
    f.service.shutdown().await;
}

#[tokio::test]
async fn headers_only_mail_is_found_on_the_server_and_fetched_on_open() {
    let f = setup().await;
    synced(&f).await;
    eventually("the headers-only folder is synced", || async {
        match folders(&f.service, MULTI_WORK_ID)
            .await
            .into_iter()
            .find(|f| f.path == MULTI_HEADERS_ONLY_FOLDER)
        {
            Some(folder) => !messages(&f.service, &folder.id).await.is_empty(),
            None => false,
        }
    })
    .await;
    let newsletters = folders(&f.service, MULTI_WORK_ID)
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
            query: SearchQuery::new("platypus", 50),
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
    assert!(server.failed.is_empty(), "{:?}", server.failed);

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
