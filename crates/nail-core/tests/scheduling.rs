//! Invitations and scheduling through the core (M3b WP85, ADR 0070)
//! against the demo-multi mock servers: iMIP invitations arriving by IMAP
//! are added to the calendar, answered (the `REPLY` leaves through the
//! outbox and the mock SMTP server), updated and cancelled by the
//! organizer; spoofed invitations are never applied; the organizer side
//! sends `REQUEST`s, records `REPLY`s and answers `COUNTER`s; free/busy
//! comes from the user's calendars and the mock CalDAV server's outbox.

use std::future::Future;
use std::time::Duration;

use nail_api::pim::PimTime;
use nail_api::pim::calendar::{Attendee, CalendarEvent, EventFields, ParticipationStatus};
use nail_api::scheduling::{
    FreeBusySource, Invitation, InvitationAction, InvitationResponse, InvitationRole,
    InvitationState, InvitationTarget, ItipIssue, ItipMethod, SchedulingResult,
    SchedulingTransport,
};
use nail_api::{AccountId, FolderRole, MessageSummary, Request, Response};
use nail_core::demo::{
    DemoMulti, MULTI_DAV_OTHER_EMAIL, MULTI_WORK_EMAIL, MULTI_WORK_ID, start_demo_multi,
};
use nail_core::{Service, ServiceOptions};
use serde_json::json;
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(30);

struct Fixture {
    service: Service,
    demo: DemoMulti,
    _dirs: (TempDir, TempDir),
}

async fn setup() -> Fixture {
    let demo = start_demo_multi().await.unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    demo.write_config(config_dir.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    let service = Service::open(options).unwrap();
    let f = Fixture {
        service,
        demo,
        _dirs: (data_dir, config_dir),
    };
    eventually("the work inbox synced", || async {
        inbox(&f).await.len() >= 15
    })
    .await;
    f
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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Polls until `check` yields a value.
async fn until_some<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn ok(service: &Service, request: Request) -> Response {
    let response = service.handle(request).await;
    if let Response::Error { message, .. } = &response {
        panic!("request failed: {message}");
    }
    response
}

async fn inbox(f: &Fixture) -> Vec<MessageSummary> {
    let Response::Folders { folders } = ok(
        &f.service,
        Request::ListFolders {
            account_id: AccountId::new(MULTI_WORK_ID),
        },
    )
    .await
    else {
        panic!()
    };
    let Some(inbox) = folders.into_iter().find(|f| f.role == FolderRole::Inbox) else {
        return Vec::new();
    };
    match ok(
        &f.service,
        Request::ListMessages {
            folder_id: inbox.id,
            offset: 0,
            limit: 500,
        },
    )
    .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("{other:?}"),
    }
}

/// Delivers a mail to the work inbox and waits until Nail has it.
async fn deliver(f: &Fixture, raw: &str, subject: &str) -> MessageSummary {
    f.demo
        .control("imap-deliver", &json!({ "raw": raw }))
        .unwrap();
    until_some(&format!("{subject:?} arrives"), || async {
        inbox(f).await.into_iter().find(|m| m.subject == subject)
    })
    .await
}

async fn card(f: &Fixture, message: &MessageSummary) -> Invitation {
    match ok(
        &f.service,
        Request::GetInvitation {
            message_id: message.id.clone(),
        },
    )
    .await
    {
        Response::Invitation {
            invitation: Some(card),
        } => *card,
        other => panic!("no card: {other:?}"),
    }
}

async fn event(f: &Fixture, card: &Invitation) -> Option<CalendarEvent> {
    let id = card.event_id.clone()?;
    match f.service.handle(Request::GetEvent { event_id: id }).await {
        Response::PimEvent { event } => Some(*event),
        _ => None,
    }
}

fn scheduled(response: Response) -> SchedulingResult {
    match response {
        Response::Scheduled { result } => *result,
        other => panic!("{other:?}"),
    }
}

/// What the mock SMTP server received (raw texts).
fn smtp(f: &Fixture) -> Vec<String> {
    f.demo
        .control("smtp-messages", &json!({}))
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["data"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// An iMIP mail.
#[allow(clippy::too_many_arguments)]
fn imip(
    from: &str,
    to: &str,
    subject: &str,
    message_id: &str,
    method: &str,
    uid: &str,
    sequence: u32,
    stamp: &str,
    body: &str,
) -> String {
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: Thu, 01 Oct 2026 08:00:00 +0000\r\n\
         Message-ID: <{message_id}>\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/calendar; method={method}; charset=utf-8\r\n\r\n\
         BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nMETHOD:{method}\r\nBEGIN:VEVENT\r\n\
         UID:{uid}\r\nDTSTAMP:{stamp}\r\nSEQUENCE:{sequence}\r\n{body}END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

const GRACE: &str = "Grace Hopper <grace@example.org>";

fn invitation(uid: &str, sequence: u32, title: &str, start: &str, stamp: &str) -> String {
    imip(
        GRACE,
        MULTI_WORK_EMAIL,
        title,
        &format!("{uid}-{sequence}"),
        "REQUEST",
        uid,
        sequence,
        stamp,
        &format!(
            "DTSTART:{start}\r\nDTEND:{}\r\nSUMMARY:{title}\r\nLOCATION:Room 1\r\n\
             ORGANIZER;CN=Grace Hopper:mailto:grace@example.org\r\n\
             ATTENDEE;CN=Ada;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{MULTI_WORK_EMAIL}\r\n\
             ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n",
            start.replace("T09", "T10")
        ),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn invitations_are_added_answered_updated_and_cancelled() {
    let f = setup().await;
    let uid = "planning-1@example.org";
    let message = deliver(
        &f,
        &invitation(uid, 0, "Planning", "20261005T090000Z", "20261001T080000Z"),
        "Planning",
    )
    .await;
    // Added unanswered on arrival (settings: add_invitations).
    let first = until_some("the invitation is in the calendar", || async {
        let c = card(&f, &message).await;
        c.event_id.is_some().then_some(c)
    })
    .await;
    assert_eq!(first.method, ItipMethod::Request);
    assert_eq!(first.role, InvitationRole::Attendee);
    assert_eq!(first.my_address.as_deref(), Some(MULTI_WORK_EMAIL));
    assert_eq!(first.my_status, Some(ParticipationStatus::NeedsAction));
    assert_eq!(first.transport, SchedulingTransport::Imip);
    assert!(first.allows(InvitationAction::Accept));
    assert!(first.issues.is_empty(), "{:?}", first.issues);
    // The reader shows it too.
    let Response::Message { message: body } = ok(
        &f.service,
        Request::GetMessage {
            id: message.id.clone(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(body.invitation.as_ref().map(|c| c.uid.as_str()), Some(uid));

    // Accept with a comment: the copy says so, the REPLY goes out.
    let result = scheduled(
        ok(
            &f.service,
            Request::RespondToInvitation {
                target: InvitationTarget::Message {
                    message_id: message.id.clone(),
                },
                response: InvitationResponse::Accept,
                comment: Some("See you there".into()),
                send: true,
                calendar_id: None,
            },
        )
        .await,
    );
    assert_eq!(result.sent.len(), 1);
    assert_eq!(result.sent[0].method, ItipMethod::Reply);
    assert!(result.sent[0].outbox_id.is_some(), "undo send applies");
    let copy = result.event.unwrap();
    let me = copy
        .fields
        .attendees
        .iter()
        .find(|a| a.email == MULTI_WORK_EMAIL)
        .unwrap();
    assert_eq!(me.status, ParticipationStatus::Accepted);
    assert_eq!(
        result.invitation.unwrap().my_status,
        Some(ParticipationStatus::Accepted)
    );
    eventually("the REPLY reaches the SMTP server", || async {
        smtp(&f).iter().any(|m| {
            m.contains("METHOD:REPLY")
                && m.contains("PARTSTAT=ACCEPTED")
                && m.contains("See you there")
                && m.contains("To: \"Grace Hopper\" <grace@example.org>")
        })
    })
    .await;

    // The organizer moves the meeting: applied on arrival.
    let update = deliver(
        &f,
        &invitation(
            uid,
            1,
            "Planning (moved)",
            "20261006T090000Z",
            "20261002T080000Z",
        ),
        "Planning (moved)",
    )
    .await;
    eventually("the update is applied", || async {
        let c = card(&f, &update).await;
        event(&f, &c)
            .await
            .is_some_and(|e| e.fields.title == "Planning (moved)" && e.sequence == 1)
    })
    .await;
    // The first message is now outdated.
    assert_eq!(card(&f, &message).await.state, InvitationState::Outdated);

    // The organizer cancels: the card offers the removal (settings do not
    // apply cancellations on their own).
    let cancel = deliver(
        &f,
        &imip(
            GRACE,
            MULTI_WORK_EMAIL,
            "Cancelled: Planning",
            "planning-cancel",
            "CANCEL",
            uid,
            2,
            "20261003T080000Z",
            &format!(
                "ORGANIZER:mailto:grace@example.org\r\nATTENDEE:mailto:{MULTI_WORK_EMAIL}\r\nSTATUS:CANCELLED\r\n"
            ),
        ),
        "Cancelled: Planning",
    )
    .await;
    let c = card(&f, &cancel).await;
    assert!(c.allows(InvitationAction::RemoveFromCalendar), "{c:?}");
    let event_id = c.event_id.clone().unwrap();
    let result = scheduled(
        ok(
            &f.service,
            Request::ApplyInvitation {
                message_id: cancel.id.clone(),
                calendar_id: None,
            },
        )
        .await,
    );
    assert_eq!(result.removed_event_id, Some(event_id.clone()));
    assert!(matches!(
        f.service.handle(Request::GetEvent { event_id }).await,
        Response::Error { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn spoofed_invitations_are_never_applied() {
    let f = setup().await;
    let message = deliver(
        &f,
        &imip(
            "Mallory <mallory@evil.example>",
            MULTI_WORK_EMAIL,
            "Urgent meeting",
            "spoof-1",
            "REQUEST",
            "spoof-1@example.org",
            0,
            "20261001T080000Z",
            &format!(
                "DTSTART:20261007T090000Z\r\nDTEND:20261007T100000Z\r\nSUMMARY:Urgent meeting\r\n\
                 ORGANIZER:mailto:ceo@work.example\r\nATTENDEE;RSVP=TRUE:mailto:{MULTI_WORK_EMAIL}\r\n"
            ),
        ),
        "Urgent meeting",
    )
    .await;
    // Give arrival processing a moment, then check nothing was added.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let c = card(&f, &message).await;
    assert!(c.issues.contains(&ItipIssue::SenderNotOrganizer), "{c:?}");
    assert_eq!(c.event_id, None);
    // The user may still add it on purpose.
    assert!(c.allows(InvitationAction::Accept));
}

#[tokio::test(flavor = "multi_thread")]
async fn organizers_invite_record_replies_and_answer_counters() {
    let f = setup().await;
    // A meeting in the local calendar: the organizer is filled in.
    let Response::Calendars { calendars } =
        ok(&f.service, Request::ListCalendars { source_id: None }).await
    else {
        panic!()
    };
    let local = calendars
        .iter()
        .find(|c| c.source_id.as_str() == "local" && !c.read_only)
        .unwrap();
    let mut fields = EventFields::new(
        "Design review",
        PimTime::utc("2026-10-08T13:00:00"),
        PimTime::utc("2026-10-08T14:00:00"),
    );
    let mut grace = Attendee::new("grace@example.org");
    grace.name = Some("Grace Hopper".into());
    fields.attendees = vec![grace];
    let Response::EventSaved { event, .. } = ok(
        &f.service,
        Request::CreateEvent {
            calendar_id: local.id.clone(),
            fields: Box::new(fields),
            recurrence: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(
        event.fields.organizer.as_ref().map(|o| o.email.as_str()),
        Some(MULTI_WORK_EMAIL)
    );
    let result = scheduled(
        ok(
            &f.service,
            Request::SendInvitations {
                event_id: event.id.clone(),
                recurrence_id: None,
                attendees: Vec::new(),
                comment: None,
            },
        )
        .await,
    );
    assert_eq!(result.sent[0].recipients, ["grace@example.org"]);
    eventually("the REQUEST reaches the SMTP server", || async {
        smtp(&f).iter().any(|m| {
            m.contains("METHOD:REQUEST")
                && m.contains(&format!("UID:{}", event.uid))
                && m.contains("invite.ics")
        })
    })
    .await;

    // Grace accepts: recorded in the organizer's copy on arrival.
    let reply = deliver(
        &f,
        &imip(
            GRACE,
            MULTI_WORK_EMAIL,
            "Accepted: Design review",
            "reply-1",
            "REPLY",
            &event.uid,
            0,
            "20261001T090000Z",
            &format!(
                "ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n"
            ),
        ),
        "Accepted: Design review",
    )
    .await;
    eventually("the reply is recorded", || async {
        let Response::PimEvent { event } = f
            .service
            .handle(Request::GetEvent {
                event_id: event.id.clone(),
            })
            .await
        else {
            return false;
        };
        event.fields.attendees[0].status == ParticipationStatus::Accepted
    })
    .await;
    assert_eq!(card(&f, &reply).await.role, InvitationRole::Organizer);

    // Grace proposes another time; the organizer accepts it.
    let counter = deliver(
        &f,
        &imip(
            GRACE,
            MULTI_WORK_EMAIL,
            "New time proposed: Design review",
            "counter-1",
            "COUNTER",
            &event.uid,
            0,
            "20261001T100000Z",
            &format!(
                "DTSTART:20261008T150000Z\r\nDTEND:20261008T160000Z\r\nSUMMARY:Design review\r\n\
                 ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n\
                 COMMENT:Afternoon is better\r\n"
            ),
        ),
        "New time proposed: Design review",
    )
    .await;
    let c = card(&f, &counter).await;
    assert_eq!(c.method, ItipMethod::Counter);
    assert!(c.allows(InvitationAction::AcceptCounter), "{c:?}");
    assert_eq!(
        c.counter.as_ref().map(|p| p.start.clone()),
        Some(PimTime::utc("2026-10-08T15:00:00"))
    );
    let result = scheduled(
        ok(
            &f.service,
            Request::RespondToCounter {
                message_id: counter.id.clone(),
                accept: true,
                comment: None,
            },
        )
        .await,
    );
    let moved = result.event.unwrap();
    assert_eq!(moved.fields.start, PimTime::utc("2026-10-08T15:00:00"));
    assert!(moved.sequence >= 1);
    assert_eq!(result.sent[0].method, ItipMethod::Request);
    eventually("the updated REQUEST goes out", || async {
        smtp(&f)
            .iter()
            .any(|m| m.contains("METHOD:REQUEST") && m.contains("DTSTART:20261008T150000Z"))
    })
    .await;

    // Cancelling the meeting tells Grace and removes it.
    let result = scheduled(
        ok(
            &f.service,
            Request::CancelMeeting {
                event_id: event.id.clone(),
                scope: nail_api::scheduling::CancelScope::Meeting,
                recurrence_id: None,
                attendees: Vec::new(),
                comment: Some("Sorry".into()),
            },
        )
        .await,
    );
    assert_eq!(result.removed_event_id, Some(event.id.clone()));
    eventually("the CANCEL goes out", || async {
        smtp(&f)
            .iter()
            .any(|m| m.contains("METHOD:CANCEL") && m.contains(&event.uid))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn free_busy_comes_from_own_calendars_and_the_caldav_outbox() {
    let f = setup().await;
    let day = chrono::Utc::now().date_naive() + chrono::Duration::days(1);
    let from = format!("{}T00:00:00Z", day.format("%Y-%m-%d"));
    let to = format!(
        "{}T00:00:00Z",
        (day + chrono::Duration::days(1)).format("%Y-%m-%d")
    );
    let result = until_some("the CalDAV source offers free/busy", || async {
        let Response::FreeBusy { result: r } = f
            .service
            .handle(Request::QueryFreeBusy {
                attendees: vec![
                    MULTI_WORK_EMAIL.into(),
                    MULTI_DAV_OTHER_EMAIL.into(),
                    "nobody@nowhere.example".into(),
                ],
                from: from.clone(),
                to: to.clone(),
                identity_id: None,
                ignore_event_id: None,
            })
            .await
        else {
            return None;
        };
        matches!(r.attendees[1].source, FreeBusySource::Caldav { .. }).then_some(r)
    })
    .await;
    assert_eq!(result.attendees.len(), 3);
    assert_eq!(result.attendees[0].source, FreeBusySource::Own);
    // The demo "Quarterly review" (tomorrow 14:00 UTC) is the user's.
    assert!(!result.attendees[0].periods.is_empty(), "{result:?}");
    let bob = &result.attendees[1];
    assert_eq!(bob.periods.len(), 2, "{bob:?}");
    assert!(bob.periods[0].start.ends_with("T10:00:00Z"));
    assert_eq!(result.attendees[2].source, FreeBusySource::Unknown);
    // Bad queries name their problem.
    let Response::Error { localized, .. } = f
        .service
        .handle(Request::QueryFreeBusy {
            attendees: vec![],
            from,
            to,
            identity_id: None,
            ignore_event_id: None,
        })
        .await
    else {
        panic!()
    };
    assert_eq!(
        localized.map(|l| l.id),
        Some("freebusy-issue-attendee-count".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_round_trip() {
    let f = setup().await;
    let Response::SchedulingSettings { mut settings } =
        ok(&f.service, Request::GetSchedulingSettings).await
    else {
        panic!()
    };
    assert!(settings.add_invitations && !settings.apply_cancellations);
    settings.apply_cancellations = true;
    ok(
        &f.service,
        Request::SetSchedulingSettings {
            settings: settings.clone(),
        },
    )
    .await;
    let Response::SchedulingSettings { settings: again } =
        ok(&f.service, Request::GetSchedulingSettings).await
    else {
        panic!()
    };
    assert_eq!(again, settings);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_schedules_delivers_and_nail_mails_nothing() {
    let demo = start_demo_multi().await.unwrap();
    // RFC 6638: the mock server schedules itself (calendar-auto-schedule).
    demo.dav.set_auto_schedule(true);
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    demo.write_config(config_dir.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    let service = Service::open(options).unwrap();
    let f = Fixture {
        service,
        demo,
        _dirs: (data_dir, config_dir),
    };
    // The CalDAV calendar, once discovered (with its scheduling).
    let calendar = until_some("the CalDAV calendar", || async {
        let Response::Calendars { calendars } = f
            .service
            .handle(Request::ListCalendars { source_id: None })
            .await
        else {
            return None;
        };
        calendars
            .into_iter()
            .find(|c| c.source_id.as_str() == "work-calendars" && !c.read_only)
    })
    .await;
    let mut fields = EventFields::new(
        "Server-scheduled",
        PimTime::utc("2026-10-09T09:00:00"),
        PimTime::utc("2026-10-09T10:00:00"),
    );
    fields.attendees = vec![Attendee::new(MULTI_DAV_OTHER_EMAIL)];
    let Response::EventSaved { event, .. } = ok(
        &f.service,
        Request::CreateEvent {
            calendar_id: calendar.id.clone(),
            fields: Box::new(fields),
            recurrence: None,
        },
    )
    .await
    else {
        panic!()
    };
    // Writing the event made the server deliver the REQUEST to bob.
    eventually("the server delivers the REQUEST", || async {
        f.demo
            .dav
            .scheduled_messages()
            .iter()
            .any(|m| m.method == "REQUEST" && m.uid == event.uid && m.to == MULTI_DAV_OTHER_EMAIL)
    })
    .await;
    let result = scheduled(
        ok(
            &f.service,
            Request::SendInvitations {
                event_id: event.id.clone(),
                recurrence_id: None,
                attendees: Vec::new(),
                comment: None,
            },
        )
        .await,
    );
    assert_eq!(result.sent.len(), 1);
    assert_eq!(result.sent[0].transport, SchedulingTransport::Server);
    assert_eq!(result.sent[0].outbox_id, None);
    // Never both: no iMIP mail for this event.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!smtp(&f).iter().any(|m| m.contains(&event.uid)));
}
