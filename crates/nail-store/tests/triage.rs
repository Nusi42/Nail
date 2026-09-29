//! Schema v7 (ADR 0043): body state, snooze, junk classifier data,
//! notification settings and the Sieve state.

mod common;

use common::{folder, message, seeded_store};
use nail_api::sieve::{VacationIssue, VacationSettings};
use nail_api::sync::BodyState;
use nail_api::triage::{
    JunkSettings, JunkSource, JunkVerdict, NotificationOverride, NotificationScope,
    NotificationSettings, NotifyFolders, QuietHours, Weekday,
};
use nail_api::{AccountId, ApiError, ErrorCode, FolderId, FolderRole, MessageId};
use nail_store::{
    Error, MCP_SETTINGS_KEY, MessageMeta, SieveRecord, SyncedMessage, TrainOutcome,
    hash_junk_token, hash_sieve_script,
};
use pretty_assertions::assert_eq;

const T0: i64 = 1_790_000_000;

fn a1() -> AccountId {
    "a1".into()
}

fn with_messages() -> nail_store::Store {
    let store = seeded_store();
    store
        .merge_messages(&[SyncedMessage {
            message: message("m1", "a1", "inbox", "2026-09-01T10:00:00Z"),
            meta: Some(MessageMeta {
                message_id: Some("one@example.org".into()),
                ..MessageMeta::default()
            }),
        }])
        .unwrap();
    store
        .upsert_message(&message("m2", "a1", "inbox", "2026-09-02T10:00:00Z"))
        .unwrap();
    store
}

#[test]
fn body_state_defaults_to_synced_and_marks_summaries() {
    let store = with_messages();
    let m1 = MessageId::from("m1");
    assert_eq!(store.body_state(&m1).unwrap(), BodyState::Synced);
    assert!(!store.get_message(&m1).unwrap().summary.headers_only);

    let changed = store
        .set_body_state(&[m1.clone(), "missing".into()], BodyState::HeadersOnly)
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(store.body_state(&m1).unwrap(), BodyState::HeadersOnly);
    assert!(store.get_message(&m1).unwrap().summary.headers_only);
    let page = store.list_messages(&"inbox".into(), 0, 10).unwrap();
    let flags: Vec<(String, bool)> = page
        .messages
        .iter()
        .map(|m| (m.id.to_string(), m.headers_only))
        .collect();
    assert_eq!(flags, [("m2".into(), false), ("m1".into(), true)]);
    assert_eq!(
        store.headers_only_messages(&"inbox".into(), 10).unwrap(),
        std::slice::from_ref(&m1)
    );

    // A sync update keeps the state; fetching on demand changes it.
    store
        .upsert_message(&message("m1", "a1", "inbox", "2026-09-01T10:00:00Z"))
        .unwrap();
    assert_eq!(store.body_state(&m1).unwrap(), BodyState::HeadersOnly);
    store
        .set_body_state(std::slice::from_ref(&m1), BodyState::OnDemand)
        .unwrap();
    assert!(!store.get_message(&m1).unwrap().summary.headers_only);
    assert!(matches!(
        store.body_state(&"missing".into()),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn snoozes_are_recorded_listed_and_woken() {
    let store = with_messages();
    let inbox = FolderId::from("inbox");
    let r1 = store.snooze(&"m1".into(), T0 + 3600, &inbox, T0).unwrap();
    assert_eq!(r1.account_id, a1());
    assert_eq!(r1.rfc_message_id.as_deref(), Some("one@example.org"));
    store.snooze(&"m2".into(), T0 + 60, &inbox, T0).unwrap();

    let listed: Vec<String> = store
        .list_snoozed(None)
        .unwrap()
        .iter()
        .map(|r| r.message_id.to_string())
        .collect();
    assert_eq!(listed, ["m2", "m1"]);
    assert!(
        store
            .list_snoozed(Some(&"other".into()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.next_snooze_wake().unwrap(), Some(T0 + 60));
    assert_eq!(store.due_snoozes(T0 + 59).unwrap(), []);
    let due = store.due_snoozes(T0 + 60).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].message_id, MessageId::from("m2"));

    // Snoozing again moves the wake time.
    store.snooze(&"m1".into(), T0 + 30, &inbox, T0).unwrap();
    assert_eq!(store.next_snooze_wake().unwrap(), Some(T0 + 30));

    // Rebinding after a resync, then waking.
    assert!(store.rebind_snooze(&"m1".into(), &"m1b".into()).unwrap());
    assert!(!store.rebind_snooze(&"nope".into(), &"x".into()).unwrap());
    let woken = store
        .unsnooze(&["m1b".into(), "m2".into(), "never".into()])
        .unwrap();
    assert_eq!(woken.len(), 2);
    assert_eq!(store.next_snooze_wake().unwrap(), None);

    // Refusals: the past, a foreign folder, an unknown message.
    assert!(matches!(
        store.snooze(&"m1".into(), T0, &inbox, T0),
        Err(Error::InvalidData(_))
    ));
    store.upsert_account(&common::account("a2")).unwrap();
    store
        .upsert_folder(&folder("b-inbox", "a2", "INBOX", None, FolderRole::Inbox))
        .unwrap();
    assert!(matches!(
        store.snooze(&"m1".into(), T0 + 10, &"b-inbox".into(), T0),
        Err(Error::InvalidData(_))
    ));
    assert!(matches!(
        store.snooze(&"zz".into(), T0 + 10, &inbox, T0),
        Err(Error::NotFound { .. })
    ));

    // Deleting the account removes its snoozes.
    store.snooze(&"m1".into(), T0 + 10, &inbox, T0).unwrap();
    store.delete_account(&a1()).unwrap();
    assert!(store.list_snoozed(None).unwrap().is_empty());
}

#[test]
fn junk_training_counts_each_message_once_and_moves_between_classes() {
    let store = with_messages();
    let account = a1();
    assert_eq!(
        store.junk_settings(&account).unwrap(),
        JunkSettings::default()
    );
    let tokens = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();

    assert_eq!(
        store
            .train_junk(
                &account,
                "spam1@x",
                true,
                &tokens(&["viagra", "cheap", "cheap"]),
                T0
            )
            .unwrap(),
        TrainOutcome::Trained
    );
    assert_eq!(
        store
            .train_junk(&account, "spam1@x", true, &tokens(&["viagra"]), T0)
            .unwrap(),
        TrainOutcome::Unchanged
    );
    store
        .train_junk(
            &account,
            "ham1@x",
            false,
            &tokens(&["meeting", "cheap"]),
            T0 + 5,
        )
        .unwrap();
    let counts = store
        .junk_token_counts(
            &account,
            &tokens(&["viagra", "cheap", "meeting", "unknown"]),
        )
        .unwrap();
    assert_eq!(counts["viagra"], (1, 0));
    assert_eq!(counts["cheap"], (1, 1));
    assert_eq!(counts["meeting"], (0, 1));
    assert!(!counts.contains_key("unknown"));
    let stats = store.junk_stats(&account).unwrap();
    assert_eq!(
        (stats.trained_junk, stats.trained_ham, stats.tokens),
        (1, 1, 3)
    );
    assert_eq!(stats.last_trained_at, Some(T0 + 5));

    // "Not junk" after all: the message moves to the other class.
    assert_eq!(
        store
            .train_junk(
                &account,
                "spam1@x",
                false,
                &tokens(&["viagra", "cheap"]),
                T0 + 9
            )
            .unwrap(),
        TrainOutcome::Retrained
    );
    let counts = store
        .junk_token_counts(&account, &tokens(&["viagra", "cheap"]))
        .unwrap();
    assert_eq!(counts["viagra"], (0, 1));
    assert_eq!(counts["cheap"], (0, 2));
    let stats = store.junk_stats(&account).unwrap();
    assert_eq!((stats.trained_junk, stats.trained_ham), (0, 2));

    // Words are never stored in clear.
    assert_eq!(hash_junk_token("viagra").len(), 16);
    assert_ne!(hash_junk_token("viagra"), hash_junk_token("Viagra"));

    store.reset_junk(&account).unwrap();
    assert_eq!(
        store.junk_stats(&account).unwrap(),
        nail_store::JunkStats::default()
    );
    assert!(matches!(
        store.train_junk(&account, "", true, &[], T0),
        Err(Error::InvalidData(_))
    ));
}

#[test]
fn junk_settings_and_verdicts() {
    let store = with_messages();
    let account = a1();
    let settings = JunkSettings {
        enabled: true,
        threshold: 80,
        min_training: 2,
        ..JunkSettings::default()
    };
    store.set_junk_settings(&account, &settings).unwrap();
    assert_eq!(store.junk_settings(&account).unwrap(), settings);
    assert!(matches!(
        store.set_junk_settings(
            &account,
            &JunkSettings {
                threshold: 10,
                ..settings.clone()
            }
        ),
        Err(Error::InvalidData(_))
    ));

    let m1 = MessageId::from("m1");
    assert_eq!(store.junk_verdict(&m1).unwrap(), None);
    let verdict = JunkVerdict {
        junk: true,
        score: Some(97),
        source: JunkSource::Classifier,
    };
    store.set_junk_verdict(&m1, &verdict, T0).unwrap();
    assert_eq!(store.junk_verdict(&m1).unwrap(), Some(verdict));
    let user = JunkVerdict {
        junk: false,
        score: None,
        source: JunkSource::User,
    };
    store.set_junk_verdict(&m1, &user, T0 + 1).unwrap();
    assert_eq!(store.junk_verdict(&m1).unwrap(), Some(user.clone()));
    assert!(matches!(
        store.set_junk_verdict(&"nope".into(), &user, T0),
        Err(Error::NotFound { .. })
    ));
    assert!(matches!(
        store.set_junk_verdict(
            &m1,
            &JunkVerdict {
                score: Some(101),
                ..user
            },
            T0
        ),
        Err(Error::InvalidData(_))
    ));
    store.delete_account(&account).unwrap();
    assert_eq!(store.junk_verdict(&m1).unwrap(), None);
}

#[test]
fn notification_settings_round_trip_with_overrides() {
    let store = with_messages();
    assert_eq!(
        store.notification_settings().unwrap(),
        NotificationSettings::default()
    );
    let settings = NotificationSettings {
        enabled: true,
        show_preview: false,
        sound: false,
        folders: NotifyFolders::All,
        overrides: vec![
            NotificationOverride {
                scope: NotificationScope::Folder {
                    folder_id: "archive".into(),
                },
                notify: false,
            },
            NotificationOverride {
                scope: NotificationScope::Account { account_id: a1() },
                notify: true,
            },
        ],
        quiet_hours: Some(QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
            days: vec![Weekday::Mon, Weekday::Fri],
        }),
    };
    store.set_notification_settings(&settings).unwrap();
    assert_eq!(store.notification_settings().unwrap(), settings);

    // Unknown targets and invalid settings are refused; nothing changes.
    let mut unknown = settings.clone();
    unknown.overrides.push(NotificationOverride {
        scope: NotificationScope::Folder {
            folder_id: "ghost".into(),
        },
        notify: true,
    });
    assert!(matches!(
        store.set_notification_settings(&unknown),
        Err(Error::NotFound { .. })
    ));
    let mut invalid = settings.clone();
    invalid.quiet_hours = Some(QuietHours {
        start: "25:00".into(),
        end: "07:00".into(),
        days: vec![],
    });
    assert!(matches!(
        store.set_notification_settings(&invalid),
        Err(Error::InvalidData(_))
    ));
    assert_eq!(store.notification_settings().unwrap(), settings);

    // Deleting a folder or account removes its override.
    store.delete_folder(&"archive".into()).unwrap();
    assert_eq!(store.notification_settings().unwrap().overrides.len(), 1);
    store.delete_account(&a1()).unwrap();
    assert!(store.notification_settings().unwrap().overrides.is_empty());
}

#[test]
fn sieve_state_and_vacation() {
    let store = with_messages();
    let account = a1();
    assert_eq!(
        store.sieve_record(&account).unwrap(),
        SieveRecord::default()
    );

    let vacation = VacationSettings {
        enabled: true,
        subject: "Away".into(),
        body: "Back on Monday.".into(),
        start_date: Some("2026-10-01".into()),
        end_date: Some("2026-10-10".into()),
        ..VacationSettings::default()
    };
    store.set_vacation(&account, &vacation).unwrap();
    let invalid = VacationSettings {
        body: String::new(),
        ..vacation.clone()
    };
    assert_eq!(invalid.issues(), [VacationIssue::EmptyBody]);
    assert!(matches!(
        store.set_vacation(&account, &invalid),
        Err(Error::InvalidData(_))
    ));
    assert!(matches!(
        store.set_vacation(&"ghost".into(), &vacation),
        Err(Error::NotFound { .. })
    ));

    let error = ApiError::new(ErrorCode::Network, "connection refused");
    store.record_sieve_error(&account, Some(&error)).unwrap();
    let script = "require [\"fileinto\"];\nkeep;\n";
    store
        .record_sieve_upload(
            &account,
            script,
            &["fileinto".into(), "vacation".into()],
            &["roundcube".into()],
            T0,
        )
        .unwrap();
    let record = store.sieve_record(&account).unwrap();
    assert_eq!(
        record,
        SieveRecord {
            script_hash: Some(hash_sieve_script(script)),
            uploaded_at: Some(T0),
            extensions: vec!["fileinto".into(), "vacation".into()],
            other_scripts: vec!["roundcube".into()],
            vacation: vacation.clone(),
            last_error: None,
        }
    );
    store.record_sieve_error(&account, Some(&error)).unwrap();
    assert_eq!(
        store.sieve_record(&account).unwrap().last_error,
        Some(error)
    );
    store.record_sieve_error(&account, None).unwrap();
    assert_eq!(store.sieve_record(&account).unwrap().last_error, None);
    store.delete_account(&account).unwrap();
    assert_eq!(
        store.sieve_record(&account).unwrap(),
        SieveRecord::default()
    );
}

#[test]
fn mcp_settings_live_in_app_settings() {
    let store = seeded_store();
    let none: Option<nail_api::McpSettings> = store.setting(MCP_SETTINGS_KEY).unwrap();
    assert_eq!(none, None);
    let settings = nail_api::McpSettings { port: Some(4799) };
    store.set_setting(MCP_SETTINGS_KEY, &settings).unwrap();
    assert_eq!(store.setting(MCP_SETTINGS_KEY).unwrap(), Some(settings));
}
