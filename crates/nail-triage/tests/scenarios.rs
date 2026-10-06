//! End-to-end scenarios over the public API, the way the core (WP55) will
//! drive the crate: the M2c Playwright scenarios in miniature.
#![cfg(test)]

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use nail_api::triage::{
    JunkSettings, JunkSource, NotificationSettings, QuietHours, SNOOZE_KEYWORD,
};
use nail_api::{AccountId, FolderId, FolderRole, MessageId};
use nail_triage::classifier::{self, MessageFacts, message_key, plan_arrival, plan_mark};
use nail_triage::notify::{self, Arrival, NotificationBatcher};
use nail_triage::snooze::SnoozeRequest;
use nail_triage::{
    ClassifierParams, Clock, FolderTarget, ManualClock, MemoryJunkStore, MemorySnoozeStore,
    SnoozeScheduler, TokenSource, Tokenizer, TriageOp,
};

fn tokens(from: &str, subject: &str, text: &str) -> Vec<String> {
    let headers = vec![
        ("From".to_owned(), from.to_owned()),
        ("Subject".to_owned(), subject.to_owned()),
    ];
    Tokenizer::default().tokens(TokenSource {
        headers: &headers,
        text,
    })
}

#[test]
fn marking_junk_trains_and_the_next_similar_message_lands_in_junk() {
    let store = MemoryJunkStore::new();
    let account = AccountId::from("work");
    let settings = JunkSettings {
        enabled: true,
        min_training: 2,
        ..JunkSettings::default()
    };
    let params = ClassifierParams::default();
    let spam = [
        (
            "deals@pills.example",
            "Cheap pills today",
            "Order cheap pills at http://pills.example/buy now",
        ),
        (
            "offers@pills.example",
            "Pills discount",
            "Huge discount on pills, visit https://pills.example/offer",
        ),
        (
            "win@lotto.example",
            "You are a winner",
            "Claim your lottery prize money today",
        ),
    ];
    let ham = [
        (
            "ada@work.example",
            "Project schedule",
            "Can we move the project meeting to Thursday?",
        ),
        (
            "bob@work.example",
            "Report draft",
            "Attached is the quarterly report draft for review.",
        ),
        (
            "ada@work.example",
            "Lunch",
            "Lunch with the team on Friday?",
        ),
    ];
    let mut receipts = Vec::new();
    for (i, (from, subject, text)) in spam.iter().enumerate() {
        let id = MessageId::from(format!("s{i}"));
        let key = message_key(
            Some(&format!("<s{i}@x>")),
            &id,
            &tokens(from, subject, text),
        );
        receipts.push(
            classifier::train(
                &store,
                &account,
                &key,
                true,
                &tokens(from, subject, text),
                0,
            )
            .unwrap(),
        );
        // The user's mark also plans keyword + move to Junk.
        let facts = MessageFacts {
            message_id: Some(&id),
            folder_role: FolderRole::Inbox,
            keywords: &[],
        };
        let ops = plan_mark(&settings, &facts, &account, true);
        assert!(ops.iter().any(|o| matches!(
            o,
            TriageOp::Move {
                to: FolderTarget::Role {
                    role: FolderRole::Junk,
                    ..
                },
                ..
            }
        )));
    }
    for (i, (from, subject, text)) in ham.iter().enumerate() {
        let key = message_key(None, &MessageId::from(format!("h{i}")), &[]);
        classifier::train(
            &store,
            &account,
            &key,
            false,
            &tokens(from, subject, text),
            0,
        )
        .unwrap();
    }

    // A new, similar message arrives.
    let arriving = tokens(
        "sales@pills.example",
        "Cheap pills discount",
        "Best pills, order at http://pills.example/cheap",
    );
    let class = classifier::classify(&store, &params, &account, &settings, &arriving).unwrap();
    let id = MessageId::from("new");
    let facts = MessageFacts {
        message_id: Some(&id),
        folder_role: FolderRole::Inbox,
        keywords: &[],
    };
    let decision = plan_arrival(&settings, &facts, &account, Some(&class));
    let verdict = decision.verdict.expect("scored");
    assert!(verdict.junk, "{class:?}");
    assert_eq!(verdict.source, JunkSource::Classifier);
    assert!(decision.ops.iter().any(|o| matches!(
        o,
        TriageOp::Move {
            to: FolderTarget::Role {
                role: FolderRole::Junk,
                ..
            },
            ..
        }
    )));

    // A work message stays.
    let work = tokens(
        "ada@work.example",
        "Project meeting",
        "About the report draft review",
    );
    let class = classifier::classify(&store, &params, &account, &settings, &work).unwrap();
    let decision = plan_arrival(&settings, &facts, &account, Some(&class));
    assert!(!decision.verdict.unwrap().junk);
    assert!(decision.ops.is_empty(), "{:?}", decision.ops);

    // Undoing the junk training takes the classifier back below readiness.
    for r in &receipts {
        classifier::undo(&store, r, &[], 1).unwrap();
    }
    let class = classifier::classify(&store, &params, &account, &settings, &arriving).unwrap();
    assert!(class.verdict().is_none());
}

#[test]
fn snooze_hides_and_returns_at_the_wake_time_across_a_restart() {
    let t0 = Utc.with_ymd_and_hms(2026, 10, 2, 9, 0, 0).unwrap();
    let clock = Arc::new(ManualClock::new(t0));
    let store = MemorySnoozeStore::new();
    let mut scheduler = SnoozeScheduler::load(store.clone(), clock.clone()).unwrap();
    let until = (t0 + Duration::hours(3)).timestamp();
    let plan = scheduler
        .snooze(&[SnoozeRequest {
            message_id: "m1".into(),
            account_id: "work".into(),
            folder_id: "inbox".into(),
            rfc_message_id: Some("<m1@example>".into()),
            until,
            wake_folder_id: None,
        }])
        .unwrap();
    assert!(plan.ops.contains(&TriageOp::AddKeyword {
        message_ids: vec!["m1".into()],
        keyword: SNOOZE_KEYWORD.into(),
    }));
    assert!(plan.ops.contains(&TriageOp::Move {
        message_ids: vec!["m1".into()],
        to: FolderTarget::Snoozed {
            account_id: "work".into()
        },
    }));
    // Restart an hour later.
    drop(scheduler);
    clock.advance(Duration::hours(1));
    let mut scheduler = SnoozeScheduler::load(store, clock.clone()).unwrap();
    let exists = |_: &FolderId| true;
    assert!(scheduler.poll(&exists).unwrap().is_empty());
    assert_eq!(scheduler.next_wake(), Some(until));
    clock.advance(Duration::hours(2));
    let woke = scheduler.poll(&exists).unwrap();
    assert_eq!(woke.woken.len(), 1);
    assert!(woke.ops.contains(&TriageOp::Move {
        message_ids: vec!["m1".into()],
        to: FolderTarget::Folder("inbox".into()),
    }));

    // It notifies (unless quiet).
    let settings = NotificationSettings::default();
    let local: DateTime<Utc> = clock.now();
    let n = notify::snooze_woke(
        &settings,
        Some(&"work".into()),
        Some(&"inbox".into()),
        &[notify::WokeMessage {
            message_id: "m1".into(),
            sender: "Ada".into(),
            subject: "Later".into(),
        }],
        &local,
        1,
    );
    assert!(n.is_some());
}

#[test]
fn new_mail_notifies_and_quiet_hours_suppress_it() {
    let mut settings = NotificationSettings::default();
    let arrival = Arrival {
        message_id: "m1".into(),
        account_id: "work".into(),
        account_name: "Work".into(),
        folder_id: "inbox".into(),
        folder_role: FolderRole::Inbox,
        sender: "Ada".into(),
        subject: "Hi".into(),
        rule_notify: None,
        junk: false,
    };
    let noon = Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap();
    let mut batcher = NotificationBatcher::default();
    let now = noon.timestamp();
    batcher.push(
        &settings,
        &notify::notifying(&settings, std::slice::from_ref(&arrival), &noon),
        now,
    );
    let out = batcher.flush(batcher.next_due().unwrap());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].title.id, "notification-new-mail-title");

    settings.quiet_hours = Some(QuietHours {
        start: "11:30".into(),
        end: "12:30".into(),
        days: vec![],
    });
    assert_eq!(notify::notifying(&settings, &[arrival], &noon).len(), 0);
}
