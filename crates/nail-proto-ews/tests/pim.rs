//! The EWS engine against the mock EWS server: categories, automatic
//! replies, inbox rules, rooms and the directory, free/busy, calendars
//! (series, occurrences, exceptions, meeting responses), contacts and
//! tasks.

use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::engine::FreeBusyProvider;
use nail_api::exchange::engine::{
    ExchangeDirectoryEngine, ExchangePimEngine, ExchangePimWrite, ExchangePimWriteOutcome,
    ExchangeSettingsEngine, MeetingSend, PimDelta, PimDeltaOutcome,
};
use nail_api::exchange::oof::{AutomaticRepliesSettings, AutomaticRepliesStatus, ExternalAudience};
use nail_api::exchange::pim::{
    AttendeeKind, EventKind, ExchangeAttendee, ExchangeBody, ExchangeContact, ExchangeDateTime,
    ExchangeEmail, ExchangeEvent, ExchangeLocation, ExchangePimFolder, ExchangePimItem,
    ExchangeResponse, ExchangeTask, ExchangeTaskStatus, PimFolderKind, Sensitivity, ShowAs,
};
use nail_api::exchange::rules::{ExchangeInboxRule, ExchangeRuleActions, ExchangeRuleConditions};
use nail_api::exchange::{CategoryColor, ExchangeCategorySettings, ExchangeCollectionRole};
use nail_api::scheduling::{BusyKind, FreeBusyQuery, FreeBusySource, InvitationResponse};
use nail_proto_ews::client::{CALL_TIMEOUT, lane_http};
use nail_proto_ews::testing::{
    DELEGATE_ADDRESS, MockEwsServer, OWN_ADDRESS, PRIVATE_ADDRESS, ROOM_LIST, ROOMS,
    SHARED_ADDRESS, fixtures,
};
use nail_proto_ews::types::FolderRef;
use nail_proto_ews::{EwsAuth, EwsClient, EwsEngine, RetryPolicy};
use pretty_assertions::assert_eq;

fn engine(server: &MockEwsServer) -> EwsEngine {
    let client = EwsClient::with_http(
        lane_http(CALL_TIMEOUT).unwrap(),
        lane_http(Duration::from_secs(120)).unwrap(),
        &server.url(),
        EwsAuth::None,
    )
    .unwrap()
    .with_own_address(OWN_ADDRESS)
    .with_retry(RetryPolicy::fast());
    EwsEngine::new(client, "work".into())
}

fn delta(outcome: PimDeltaOutcome) -> PimDelta {
    match outcome {
        PimDeltaOutcome::Delta(delta) => delta,
        PimDeltaOutcome::CursorExpired => panic!("unexpected CursorExpired"),
    }
}

fn events(delta: &PimDelta) -> Vec<&ExchangeEvent> {
    delta
        .upserted
        .iter()
        .filter_map(|i| match i {
            ExchangePimItem::Event(e) => Some(e.as_ref()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Settings

#[tokio::test]
async fn categories_live_in_the_category_list() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let list = engine.categories().await.unwrap();
    assert_eq!(list.len(), 3);
    let work = list.iter().find(|c| c.name == "Work").unwrap();
    assert_eq!(work.color, CategoryColor::Preset7);

    let id = engine
        .create_category(&ExchangeCategorySettings {
            name: "Travel".into(),
            color: CategoryColor::Preset3,
        })
        .await
        .unwrap();
    let list = engine.categories().await.unwrap();
    assert_eq!(list.len(), 4);
    assert!(
        list.iter()
            .any(|c| c.id == id && c.color == CategoryColor::Preset3)
    );
    let err = engine
        .create_category(&ExchangeCategorySettings::named("travel"))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    engine
        .update_category(
            &id,
            &ExchangeCategorySettings {
                name: "Trips".into(),
                color: CategoryColor::None,
            },
        )
        .await
        .unwrap();
    let document = server.category_list().unwrap();
    assert!(document.contains(r#"name="Trips""#) && document.contains(r#"color="-1""#));
    // Attributes Outlook keeps are preserved.
    assert!(document.contains(r#"usageCount="5""#));
    assert_eq!(
        engine
            .update_category(
                "{00000000-0000-0000-0000-000000000000}",
                &ExchangeCategorySettings::named("x")
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    engine.delete_category(&id).await.unwrap();
    engine.delete_category(&id).await.unwrap();
    assert_eq!(engine.categories().await.unwrap().len(), 3);

    // A mailbox without a list yet.
    server.set_category_list(None);
    assert!(engine.categories().await.unwrap().is_empty());
    engine
        .create_category(&ExchangeCategorySettings::named("First"))
        .await
        .unwrap();
    assert!(
        server
            .operations()
            .contains(&"CreateUserConfiguration".to_owned())
    );
    assert_eq!(engine.categories().await.unwrap()[0].name, "First");
}

#[tokio::test]
async fn automatic_replies_round_trip() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    assert_eq!(
        engine.automatic_replies().await.unwrap(),
        AutomaticRepliesSettings::default()
    );
    let settings = AutomaticRepliesSettings {
        status: AutomaticRepliesStatus::Scheduled,
        start: Some("2026-10-12T06:00:00Z".into()),
        end: Some("2026-10-16T16:00:00Z".into()),
        internal_reply: "At the conference.\nBob covers for me.".into(),
        external_reply: "Out of office until 16 October.".into(),
        external_audience: ExternalAudience::ContactsOnly,
    };
    engine.set_automatic_replies(&settings).await.unwrap();
    let stored = server.oof();
    assert_eq!(stored.state, "Scheduled");
    assert_eq!(stored.audience, "Known");
    assert!(stored.internal.contains("<br>"));
    assert_eq!(engine.automatic_replies().await.unwrap(), settings);
    let request = server
        .requests()
        .into_iter()
        .find(|r| r.operation == "SetUserOofSettingsRequest")
        .unwrap();
    assert!(request.body.contains(OWN_ADDRESS));

    // Outlook's HTML from a recorded answer becomes text.
    server.answer_with("GetUserOofSettingsRequest", fixtures::RECORDED_OOF, 1);
    let recorded = engine.automatic_replies().await.unwrap();
    assert_eq!(recorded.status, AutomaticRepliesStatus::Scheduled);
    assert_eq!(
        recorded.internal_reply,
        "I am at the conference until Friday.\nBob covers for me."
    );
    assert_eq!(recorded.external_reply, "Out of office until 16 October.");
    assert_eq!(recorded.end.as_deref(), Some("2026-10-16T16:00:00Z"));
}

fn nail_rule() -> ExchangeInboxRule {
    ExchangeInboxRule {
        id: String::new(),
        name: "Receipts".into(),
        sequence: 3,
        enabled: true,
        conditions: ExchangeRuleConditions {
            from_addresses: vec!["orders@shop.example".into()],
            subject_contains: vec!["order".into()],
            ..ExchangeRuleConditions::default()
        },
        exceptions: ExchangeRuleConditions::default(),
        actions: ExchangeRuleActions {
            move_to_folder: Some("AQMkReceipts".into()),
            assign_categories: vec!["Personal".into()],
            stop_processing_rules: true,
            ..ExchangeRuleActions::default()
        },
        has_unsupported: false,
        in_error: false,
        nail_rule_id: None,
    }
}

#[tokio::test]
async fn inbox_rules_are_listed_created_and_changed() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let rules = engine.inbox_rules().await.unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].name, "Newsletters");
    assert!(!rules[0].has_unsupported);
    assert_eq!(rules[0].actions.assign_categories, ["Personal"]);
    let voicemail = rules[1].clone();
    assert!(voicemail.has_unsupported);

    let id = engine.create_inbox_rule(&nail_rule()).await.unwrap();
    let rules = engine.inbox_rules().await.unwrap();
    let created = rules.iter().find(|r| r.id == id).unwrap();
    assert_eq!(created.actions.move_to_folder, Some("AQMkReceipts".into()));
    assert_eq!(created.conditions.subject_contains, ["order"]);

    let mut changed = created.clone();
    changed.enabled = false;
    changed.actions.mark_as_read = true;
    engine.update_inbox_rule(&changed).await.unwrap();
    let rules = engine.inbox_rules().await.unwrap();
    let after = rules.iter().find(|r| r.id == id).unwrap();
    assert!(!after.enabled && after.actions.mark_as_read);

    // A rule Nail cannot express keeps its server XML.
    let mut off = voicemail.clone();
    off.enabled = false;
    engine.update_inbox_rule(&off).await.unwrap();
    let raw = server
        .rules()
        .into_iter()
        .find(|r| r.child_text("RuleId") == Some(voicemail.id.as_str()))
        .unwrap();
    assert_eq!(raw.child_text("IsEnabled"), Some("false"));
    assert!(raw.path(&["Conditions", "IsVoicemail"]).is_some());

    engine.delete_inbox_rule(&id).await.unwrap();
    engine.delete_inbox_rule(&id).await.unwrap();
    assert_eq!(engine.inbox_rules().await.unwrap().len(), 2);
    let mut missing = nail_rule();
    missing.id = "AQAAAJ5Rnope".into();
    assert_eq!(
        engine.update_inbox_rule(&missing).await.unwrap_err().code,
        ErrorCode::NotFound
    );

    // Outlook's client rules are not deleted silently.
    server.set_outlook_rule_blob(true);
    let err = engine.create_inbox_rule(&nail_rule()).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.localized.unwrap().id, "ews-outlook-rule-blob");
    let forced = engine.with_options(nail_proto_ews::EwsOptions {
        remove_outlook_rules: true,
        ..nail_proto_ews::EwsOptions::default()
    });
    forced.create_inbox_rule(&nail_rule()).await.unwrap();
}

// ---------------------------------------------------------------------------
// Directory and free/busy

#[tokio::test]
async fn rooms_mailboxes_and_delegates() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let lists = engine.room_lists().await.unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].address, ROOM_LIST);
    assert_eq!(lists[0].name, "Headquarters");
    let all = engine.rooms(None).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].address, ROOMS[0]);
    assert_eq!(all[0].room_list.as_deref(), Some(ROOM_LIST));
    assert_eq!(engine.rooms(Some(ROOM_LIST)).await.unwrap().len(), 2);
    assert_eq!(
        engine
            .rooms(Some("nolist@corp.example"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    server.deny_rooms(true);
    assert_eq!(
        engine.room_lists().await.unwrap_err().code,
        ErrorCode::Forbidden
    );

    assert_eq!(
        engine
            .check_mailbox(SHARED_ADDRESS)
            .await
            .unwrap()
            .as_deref(),
        Some("Support")
    );
    assert_eq!(
        engine
            .check_mailbox(PRIVATE_ADDRESS)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        engine
            .check_mailbox("nobody@corp.example")
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );

    let delegates = engine.client().get_delegates().await.unwrap();
    assert_eq!(delegates.len(), 1);
    assert_eq!(delegates[0].address, "bob@corp.example");
    assert_eq!(
        delegates[0].permissions[0],
        ("Calendar".to_owned(), "Editor".to_owned())
    );
    assert!(delegates[0].receives_meeting_messages);
    let (address, name) = engine.client().resolve_name("Carol").await.unwrap();
    assert_eq!(address, "carol@corp.example");
    assert_eq!(name.as_deref(), Some("Carol Shaw"));
}

#[tokio::test]
async fn free_busy_of_people_and_rooms() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    engine.rooms(None).await.unwrap();
    server.set_free_busy(
        ROOMS[0],
        &[("2026-10-05T08:00:00Z", "2026-10-05T09:00:00Z", "Busy")],
    );
    let answer = engine
        .free_busy(&FreeBusyQuery {
            organizer: OWN_ADDRESS.into(),
            attendees: vec![
                OWN_ADDRESS.into(),
                ROOMS[0].into(),
                "nobody@corp.example".into(),
                DELEGATE_ADDRESS.into(),
            ],
            from: "2026-10-05T00:00:00Z".into(),
            to: "2026-10-13T00:00:00Z".into(),
        })
        .await
        .unwrap();
    assert_eq!(answer.len(), 3, "unknown attendees are left out");
    let own = &answer[0];
    assert_eq!(own.email, OWN_ADDRESS);
    assert_eq!(
        own.source,
        FreeBusySource::Exchange {
            account_id: "work".into()
        }
    );
    // Two Mondays of the weekly series and the tentative board review.
    let busy: Vec<(&str, BusyKind)> = own
        .periods
        .iter()
        .map(|p| (p.start.as_str(), p.kind))
        .collect();
    assert_eq!(
        busy,
        [
            ("2026-10-05T07:00:00Z", BusyKind::Busy),
            ("2026-10-07T12:00:00Z", BusyKind::Tentative),
            ("2026-10-12T07:00:00Z", BusyKind::Busy),
        ]
    );
    assert_eq!(answer[1].periods[0].end, "2026-10-05T09:00:00Z");
    assert_eq!(answer[2].email, DELEGATE_ADDRESS);
    let request = server
        .requests()
        .into_iter()
        .find(|r| r.operation == "GetUserAvailabilityRequest")
        .unwrap();
    assert!(request.body.contains("<AttendeeType>Room</AttendeeType>"));

    // A recorded answer with a person and an unknown address.
    server.answer_with(
        "GetUserAvailabilityRequest",
        fixtures::RECORDED_AVAILABILITY,
        1,
    );
    let answer = engine
        .free_busy(&FreeBusyQuery {
            organizer: OWN_ADDRESS.into(),
            attendees: vec!["bob@corp.example".into(), "nobody@corp.example".into()],
            from: "2026-10-05T00:00:00Z".into(),
            to: "2026-10-06T00:00:00Z".into(),
        })
        .await
        .unwrap();
    assert_eq!(answer.len(), 1);
    assert_eq!(answer[0].periods.len(), 2);
    assert_eq!(answer[0].periods[1].kind, BusyKind::Tentative);
}

// ---------------------------------------------------------------------------
// Calendars, contacts, tasks

fn find_folder(folders: &[ExchangePimFolder], kind: PimFolderKind) -> ExchangePimFolder {
    folders.iter().find(|f| f.kind == kind).unwrap().clone()
}

#[tokio::test]
async fn pim_folders_of_own_and_delegated_mailboxes() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let folders = engine.pim_folders(None).await.unwrap();
    assert_eq!(folders.len(), 3);
    assert_eq!(
        find_folder(&folders, PimFolderKind::Calendar).role,
        Some(ExchangeCollectionRole::DefaultCalendar)
    );
    assert_eq!(
        find_folder(&folders, PimFolderKind::Contacts).role,
        Some(ExchangeCollectionRole::DefaultContacts)
    );
    assert_eq!(
        find_folder(&folders, PimFolderKind::Tasks).role,
        Some(ExchangeCollectionRole::DefaultTasks)
    );
    // A delegate sees the PIM folders of the other mailbox only.
    let delegated = engine.pim_folders(Some(DELEGATE_ADDRESS)).await.unwrap();
    assert_eq!(delegated.len(), 3);
    let calendar = find_folder(&delegated, PimFolderKind::Calendar);
    let items = delta(
        engine
            .pim_changes(Some(DELEGATE_ADDRESS), &calendar, None)
            .await
            .unwrap(),
    );
    assert_eq!(events(&items)[0].subject, "Budget planning");
    assert_eq!(
        engine
            .pim_folders(Some(PRIVATE_ADDRESS))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn calendar_sync_reads_series_and_singles() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let calendar = find_folder(
        &engine.pim_folders(None).await.unwrap(),
        PimFolderKind::Calendar,
    );
    let first = delta(engine.pim_changes(None, &calendar, None).await.unwrap());
    let list = events(&first);
    assert_eq!(list.len(), 2);
    let series = list.iter().find(|e| e.subject == "Team sync").unwrap();
    assert_eq!(series.kind, EventKind::SeriesMaster);
    assert_eq!(series.start, ExchangeDateTime::utc("2026-10-05T07:00:00"));
    assert_eq!(
        series.recurrence.as_ref().unwrap().time_zone.as_deref(),
        Some("W. Europe Standard Time")
    );
    assert_eq!(series.attendees[0].response, ExchangeResponse::Accepted);
    assert!(series.is_organizer);
    let review = list.iter().find(|e| e.subject == "Board review").unwrap();
    assert_eq!(review.show_as, ShowAs::Tentative);
    assert_eq!(review.my_response, ExchangeResponse::NotResponded);
    assert!(!review.is_organizer);

    let quiet = delta(
        engine
            .pim_changes(None, &calendar, Some(&first.cursor))
            .await
            .unwrap(),
    );
    assert!(quiet.upserted.is_empty() && quiet.removed.is_empty());
    server.set_property(&review.remote_id, "Subject", "Board review (moved)");
    server.delete_item(&series.remote_id);
    let changes = delta(
        engine
            .pim_changes(None, &calendar, Some(&quiet.cursor))
            .await
            .unwrap(),
    );
    assert_eq!(events(&changes)[0].subject, "Board review (moved)");
    assert_eq!(changes.removed, std::slice::from_ref(&series.remote_id));
    server.expire_sync_states();
    assert_eq!(
        engine
            .pim_changes(None, &calendar, Some(&changes.cursor))
            .await
            .unwrap(),
        PimDeltaOutcome::CursorExpired
    );

    // The calendar view expands the series.
    let server = MockEwsServer::start().await.unwrap();
    let engine = self::engine(&server);
    let view = engine
        .client()
        .calendar_view(
            None,
            FolderRef::distinguished("calendar", None),
            "2026-10-01T00:00:00Z",
            "2026-10-20T00:00:00Z",
            100,
        )
        .await
        .unwrap();
    let occurrences: Vec<&ExchangeEvent> = view
        .iter()
        .filter(|e| e.kind == EventKind::Occurrence)
        .collect();
    assert_eq!(occurrences.len(), 3);
    assert_eq!(
        occurrences[1].original_start,
        Some(ExchangeDateTime::utc("2026-10-12T07:00:00"))
    );
    assert!(view.iter().any(|e| e.kind == EventKind::SingleInstance));
}

fn meeting() -> ExchangeEvent {
    let zone = "W. Europe Standard Time";
    ExchangeEvent {
        remote_id: String::new(),
        change_key: None,
        uid: Some("nail-meeting-1@corp.example".into()),
        kind: EventKind::SingleInstance,
        series_master_id: None,
        original_start: None,
        subject: "Planning".into(),
        body: Some(ExchangeBody {
            html: false,
            content: "Agenda follows.".into(),
        }),
        start: ExchangeDateTime {
            date_time: "2026-10-08T09:00:00".into(),
            time_zone: zone.into(),
        },
        end: ExchangeDateTime {
            date_time: "2026-10-08T10:00:00".into(),
            time_zone: zone.into(),
        },
        is_all_day: false,
        locations: vec![ExchangeLocation {
            display_name: "Room 1 (8 seats)".into(),
            email: Some(ROOMS[0].into()),
            room: true,
        }],
        show_as: ShowAs::Busy,
        sensitivity: Sensitivity::Normal,
        importance: None,
        reminder_minutes: Some(15),
        organizer: None,
        attendees: vec![
            ExchangeAttendee {
                email: ExchangeEmail {
                    address: "bob@corp.example".into(),
                    name: Some("Bob Builder".into()),
                },
                kind: AttendeeKind::Required,
                response: ExchangeResponse::None,
                proposed: None,
            },
            ExchangeAttendee {
                email: ExchangeEmail {
                    address: ROOMS[0].into(),
                    name: Some("Room 1 (8 seats)".into()),
                },
                kind: AttendeeKind::Resource,
                response: ExchangeResponse::None,
                proposed: None,
            },
        ],
        is_organizer: true,
        response_requested: true,
        my_response: ExchangeResponse::Organizer,
        is_cancelled: false,
        online_meeting_url: None,
        categories: vec!["Work".into()],
        recurrence: None,
        cancelled_occurrences: vec![],
        exceptions: vec![],
        last_modified: None,
    }
}

#[tokio::test]
async fn meetings_are_created_updated_and_cancelled() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let calendar = find_folder(
        &engine.pim_folders(None).await.unwrap(),
        PimFolderKind::Calendar,
    );
    let start = delta(engine.pim_changes(None, &calendar, None).await.unwrap());
    let ExchangePimWriteOutcome::Done {
        remote_id,
        change_key,
    } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: calendar.remote_id.clone(),
                item: ExchangePimItem::Event(Box::new(meeting())),
                send: MeetingSend::ToAll,
            },
        )
        .await
        .unwrap()
    else {
        panic!("created");
    };
    // Wall times in the event's zone (TimeZoneContext) are stored as UTC.
    let stored = server.item(&remote_id).unwrap();
    assert_eq!(
        stored.props.child_text("Start"),
        Some("2026-10-08T07:00:00Z")
    );
    let create = server
        .requests()
        .into_iter()
        .find(|r| r.operation == "CreateItem")
        .unwrap();
    assert_eq!(create.time_zone.as_deref(), Some("W. Europe Standard Time"));
    assert!(
        create
            .body
            .contains(r#"SendMeetingInvitations="SendToAllAndSaveCopy""#)
    );
    let invitations = server.meeting_messages();
    assert_eq!(invitations[0].kind, "Request");
    assert!(invitations[0].to.contains(&"bob@corp.example".to_owned()));
    // The room accepted and is busy.
    assert_eq!(
        server.free_busy_of(ROOMS[0]),
        [(
            "2026-10-08T07:00:00Z".to_owned(),
            "2026-10-08T08:00:00Z".to_owned(),
            "Busy".to_owned()
        )]
    );

    let read = delta(
        engine
            .pim_changes(None, &calendar, Some(&start.cursor))
            .await
            .unwrap(),
    );
    let created = events(&read)[0].clone();
    assert_eq!(created.start, ExchangeDateTime::utc("2026-10-08T07:00:00"));
    assert_eq!(created.uid.as_deref(), Some("nail-meeting-1@corp.example"));
    assert_eq!(created.categories, ["Work"]);
    let room = created
        .attendees
        .iter()
        .find(|a| a.kind == AttendeeKind::Resource)
        .unwrap();
    assert_eq!(room.response, ExchangeResponse::Accepted);
    assert_eq!(created.locations[0].email.as_deref(), Some(ROOMS[0]));

    // A stale change key is a conflict; the current one updates.
    let mut moved = meeting();
    moved.remote_id = remote_id.clone();
    moved.change_key = Some("CQAAABYstale".into());
    moved.subject = "Planning (moved)".into();
    let update = |item: ExchangeEvent| ExchangePimWrite::Update {
        item: ExchangePimItem::Event(Box::new(item)),
        send: MeetingSend::ToChanged,
    };
    assert_eq!(
        engine
            .write_pim(None, &update(moved.clone()))
            .await
            .unwrap(),
        ExchangePimWriteOutcome::Conflict
    );
    moved.change_key = change_key.clone();
    moved.start.date_time = "2026-10-08T11:00:00".into();
    moved.end.date_time = "2026-10-08T12:00:00".into();
    let ExchangePimWriteOutcome::Done {
        change_key: new_key,
        ..
    } = engine
        .write_pim(None, &update(moved.clone()))
        .await
        .unwrap()
    else {
        panic!("updated");
    };
    assert_ne!(new_key, change_key);
    let stored = server.item(&remote_id).unwrap();
    assert_eq!(stored.props.child_text("Subject"), Some("Planning (moved)"));
    assert_eq!(
        stored.props.child_text("Start"),
        Some("2026-10-08T09:00:00Z")
    );
    let update_request = server
        .requests()
        .into_iter()
        .rev()
        .find(|r| r.operation == "UpdateItem")
        .unwrap();
    assert!(
        update_request
            .body
            .contains(r#"SendMeetingInvitationsOrCancellations="SendToChangedAndSaveCopy""#)
    );
    assert_eq!(server.meeting_messages().last().unwrap().kind, "Update");

    // Deleting cancels for the attendees; a second delete finds nothing.
    let delete = ExchangePimWrite::Delete {
        remote_id: remote_id.clone(),
        send: MeetingSend::ToAll,
    };
    assert!(matches!(
        engine.write_pim(None, &delete).await.unwrap(),
        ExchangePimWriteOutcome::Done { .. }
    ));
    assert_eq!(
        server.meeting_messages().last().unwrap().kind,
        "Cancellation"
    );
    // Moved to Deleted Items, so a hard look-up still finds it there; the
    // calendar no longer has it.
    assert!(
        server
            .items_in(&calendar.remote_id)
            .iter()
            .all(|i| i.id != remote_id)
    );
    let mut gone = moved;
    gone.remote_id = "AAMkMissing".into();
    gone.change_key = None;
    assert_eq!(
        engine.write_pim(None, &update(gone)).await.unwrap(),
        ExchangePimWriteOutcome::Gone
    );
    assert_eq!(
        engine
            .write_pim(
                None,
                &ExchangePimWrite::Delete {
                    remote_id: "AAMkMissing".into(),
                    send: MeetingSend::None,
                },
            )
            .await
            .unwrap(),
        ExchangePimWriteOutcome::Gone
    );
    // IANA zones cannot be written through EWS.
    let mut iana = meeting();
    iana.start.time_zone = "Europe/Berlin".into();
    let err = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: calendar.remote_id.clone(),
                item: ExchangePimItem::Event(Box::new(iana)),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
}

#[tokio::test]
async fn series_exceptions_and_cancelled_occurrences() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let calendar = find_folder(
        &engine.pim_folders(None).await.unwrap(),
        PimFolderKind::Calendar,
    );
    let first = delta(engine.pim_changes(None, &calendar, None).await.unwrap());
    let mut series = events(&first)
        .into_iter()
        .find(|e| e.subject == "Team sync")
        .unwrap()
        .clone();
    // Cancel 12 October, move 19 October to 09:00 UTC.
    series.cancelled_occurrences = vec!["2026-10-12".into()];
    let mut exception = series.clone();
    exception.remote_id = String::new();
    exception.change_key = None;
    exception.kind = EventKind::Exception;
    exception.recurrence = None;
    exception.original_start = Some(ExchangeDateTime::utc("2026-10-19T07:00:00"));
    exception.start = ExchangeDateTime::utc("2026-10-19T09:00:00");
    exception.end = ExchangeDateTime::utc("2026-10-19T09:30:00");
    exception.subject = "Team sync (later)".into();
    series.exceptions = vec![exception];
    let ExchangePimWriteOutcome::Done { change_key, .. } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Update {
                item: ExchangePimItem::Event(Box::new(series.clone())),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("updated");
    };
    let stored = server.item(&series.remote_id).unwrap();
    assert_eq!(stored.change_key, change_key.unwrap());
    assert_eq!(stored.deleted.len(), 1);
    assert_eq!(stored.exceptions.len(), 1);

    let read = delta(
        engine
            .pim_changes(None, &calendar, Some(&first.cursor))
            .await
            .unwrap(),
    );
    let again = events(&read)
        .into_iter()
        .find(|e| e.remote_id == series.remote_id)
        .unwrap();
    assert_eq!(again.cancelled_occurrences, ["2026-10-12"]);
    assert_eq!(again.exceptions.len(), 1);
    let exception = &again.exceptions[0];
    assert_eq!(exception.kind, EventKind::Exception);
    assert_eq!(exception.subject, "Team sync (later)");
    assert_eq!(
        exception.start,
        ExchangeDateTime::utc("2026-10-19T09:00:00")
    );
    assert_eq!(
        exception.original_start,
        Some(ExchangeDateTime::utc("2026-10-19T07:00:00"))
    );
    assert_eq!(
        exception.series_master_id.as_deref(),
        Some(series.remote_id.as_str())
    );
}

#[tokio::test]
async fn meeting_responses() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let calendar = server.folder_id(None, "calendar").unwrap();
    let review = server
        .items_in(&calendar)
        .into_iter()
        .find(|i| i.props.child_text("Subject") == Some("Board review"))
        .unwrap();
    engine
        .respond_to_meeting(
            None,
            &review.id,
            InvitationResponse::Tentative,
            Some("I may be late."),
            true,
        )
        .await
        .unwrap();
    let stored = server.item(&review.id).unwrap();
    assert_eq!(stored.props.child_text("MyResponseType"), Some("Tentative"));
    let sent = server.meeting_messages();
    assert_eq!(sent[0].kind, "Tentative");
    assert_eq!(sent[0].to, [DELEGATE_ADDRESS]);
    assert_eq!(sent[0].comment.as_deref(), Some("I may be late."));
    let request = server
        .requests()
        .into_iter()
        .rev()
        .find(|r| r.operation == "CreateItem")
        .unwrap();
    assert!(request.body.contains("TentativelyAcceptItem"));
    assert!(
        request
            .body
            .contains(r#"MessageDisposition="SendAndSaveCopy""#)
    );

    // Declining without sending: nothing is sent, no draft stays behind.
    let drafts = server.folder_id(None, "drafts").unwrap();
    engine
        .respond_to_meeting(None, &review.id, InvitationResponse::Decline, None, false)
        .await
        .unwrap();
    assert_eq!(server.meeting_messages().len(), 1);
    assert!(server.items_in(&drafts).is_empty());
    assert!(server.items_in(&calendar).iter().all(|i| i.id != review.id));
    assert_eq!(
        engine
            .respond_to_meeting(None, "AAMkMissing", InvitationResponse::Accept, None, true)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn contacts_and_tasks() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let folders = engine.pim_folders(None).await.unwrap();
    let contacts = find_folder(&folders, PimFolderKind::Contacts);
    let tasks = find_folder(&folders, PimFolderKind::Tasks);

    let people = delta(engine.pim_changes(None, &contacts, None).await.unwrap());
    let mut bob = None;
    let mut team = None;
    for item in &people.upserted {
        if let ExchangePimItem::Contact(c) = item {
            if c.is_group {
                team = Some(c.as_ref().clone());
            } else {
                bob = Some(c.as_ref().clone());
            }
        }
    }
    let mut bob = bob.unwrap();
    let team = team.unwrap();
    assert_eq!(bob.display_name, "Bob Builder");
    assert_eq!(bob.emails[0].address, "bob@corp.example");
    assert_eq!(bob.phones[0].kind, "mobile");
    assert_eq!(team.members.len(), 2);

    bob.phones.push(nail_api::exchange::pim::ExchangePhone {
        kind: "business".into(),
        number: "+49 30 1234".into(),
    });
    bob.job_title = None;
    bob.addresses
        .push(nail_api::exchange::pim::ExchangePostalAddress {
            kind: "business".into(),
            city: Some("Berlin".into()),
            ..Default::default()
        });
    assert!(matches!(
        engine
            .write_pim(
                None,
                &ExchangePimWrite::Update {
                    item: ExchangePimItem::Contact(Box::new(bob.clone())),
                    send: MeetingSend::None,
                },
            )
            .await
            .unwrap(),
        ExchangePimWriteOutcome::Done { .. }
    ));
    let changed = delta(
        engine
            .pim_changes(None, &contacts, Some(&people.cursor))
            .await
            .unwrap(),
    );
    let ExchangePimItem::Contact(after) = &changed.upserted[0] else {
        panic!("a contact");
    };
    assert_eq!(after.phones.len(), 2);
    assert_eq!(after.job_title, None);
    assert_eq!(after.addresses[0].city.as_deref(), Some("Berlin"));

    let carol = ExchangeContact {
        remote_id: String::new(),
        change_key: None,
        display_name: "Carol Shaw".into(),
        given_name: Some("Carol".into()),
        surname: Some("Shaw".into()),
        middle_name: None,
        nickname: None,
        company: None,
        department: None,
        job_title: None,
        emails: vec![ExchangeEmail {
            address: "carol@corp.example".into(),
            name: None,
        }],
        phones: vec![],
        addresses: vec![],
        birthday: Some("1990-02-14".into()),
        notes: None,
        urls: vec![],
        categories: vec![],
        is_group: false,
        members: vec![],
        photo: None,
    };
    let ExchangePimWriteOutcome::Done { remote_id, .. } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: contacts.remote_id.clone(),
                item: ExchangePimItem::Contact(Box::new(carol)),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("created");
    };
    assert_eq!(server.item(&remote_id).unwrap().tag, "Contact");

    let todo = delta(engine.pim_changes(None, &tasks, None).await.unwrap());
    let ExchangePimItem::Task(task) = &todo.upserted[0] else {
        panic!("a task");
    };
    assert_eq!(task.title, "Submit expense report");
    assert_eq!(task.due, Some(ExchangeDateTime::utc("2026-10-09T22:00:00")));
    let mut done = task.as_ref().clone();
    done.status = ExchangeTaskStatus::Completed;
    done.completed = Some(ExchangeDateTime::utc("2026-10-02T10:00:00"));
    engine
        .write_pim(
            None,
            &ExchangePimWrite::Update {
                item: ExchangePimItem::Task(Box::new(done)),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap();
    let stored = server.item(&task.remote_id).unwrap();
    assert_eq!(stored.props.child_text("Status"), Some("Completed"));
    let new_task = ExchangeTask {
        remote_id: String::new(),
        change_key: None,
        title: "Book flights".into(),
        body: None,
        status: ExchangeTaskStatus::NotStarted,
        importance: None,
        start: None,
        due: Some(ExchangeDateTime {
            date_time: "2026-10-20T00:00:00".into(),
            time_zone: "W. Europe Standard Time".into(),
        }),
        completed: None,
        reminder: None,
        recurrence: None,
        categories: vec![],
        checklist: vec![],
        linked_resources: vec![],
        created: None,
        last_modified: None,
    };
    let ExchangePimWriteOutcome::Done { remote_id, .. } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: tasks.remote_id.clone(),
                item: ExchangePimItem::Task(Box::new(new_task)),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("created");
    };
    assert_eq!(
        server.item(&remote_id).unwrap().props.child_text("DueDate"),
        Some("2026-10-19T22:00:00Z")
    );
    assert!(matches!(
        engine
            .write_pim(
                None,
                &ExchangePimWrite::Delete {
                    remote_id: remote_id.clone(),
                    send: MeetingSend::None,
                },
            )
            .await
            .unwrap(),
        ExchangePimWriteOutcome::Done { .. }
    ));
    let delete = server
        .requests()
        .into_iter()
        .rev()
        .find(|r| r.operation == "DeleteItem")
        .unwrap();
    assert!(
        delete
            .body
            .contains(r#"AffectedTaskOccurrences="AllOccurrences""#)
    );
}
