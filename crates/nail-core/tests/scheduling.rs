//! Invitations and scheduling through the core (M3b WP85, ADR 0070)
//! against the demo-multi mock servers: iMIP invitations arriving by IMAP
//! are added to the calendar, answered (the `REPLY` leaves through the
//! outbox and the mock SMTP server), updated and cancelled by the
//! organizer; spoofed invitations are never applied; the organizer side
//! sends `REQUEST`s, records `REPLY`s and answers `COUNTER`s; free/busy
//! comes from the user's calendars and the mock CalDAV server's outbox.

#![cfg(test)]

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

/// The parts of an iMIP mail ([`imip`]).
struct Imip<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    message_id: &'a str,
    method: &'a str,
    uid: &'a str,
    sequence: u32,
    stamp: &'a str,
    body: &'a str,
}

/// An iMIP mail.
fn imip(mail: &Imip<'_>) -> String {
    let Imip {
        from,
        to,
        subject,
        message_id,
        method,
        uid,
        sequence,
        stamp,
        body,
    } = mail;
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
    imip(&Imip {
        from: GRACE,
        to: MULTI_WORK_EMAIL,
        subject: title,
        message_id: &format!("{uid}-{sequence}"),
        method: "REQUEST",
        uid,
        sequence,
        stamp,
        body: &format!(
            "DTSTART:{start}\r\nDTEND:{}\r\nSUMMARY:{title}\r\nLOCATION:Room 1\r\n\
             ORGANIZER;CN=Grace Hopper:mailto:grace@example.org\r\n\
             ATTENDEE;CN=Ada;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{MULTI_WORK_EMAIL}\r\n\
             ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n",
            start.replace("T09", "T10")
        ),
    })
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
        &imip(&Imip { from: GRACE, to: MULTI_WORK_EMAIL, subject: "Cancelled: Planning", message_id: "planning-cancel", method: "CANCEL", uid, sequence: 2, stamp: "20261003T080000Z", body: &format!(
                "ORGANIZER:mailto:grace@example.org\r\nATTENDEE:mailto:{MULTI_WORK_EMAIL}\r\nSTATUS:CANCELLED\r\n"
            ) }),
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
        &imip(&Imip { from: "Mallory <mallory@evil.example>", to: MULTI_WORK_EMAIL, subject: "Urgent meeting", message_id: "spoof-1", method: "REQUEST", uid: "spoof-1@example.org", sequence: 0, stamp: "20261001T080000Z", body: &format!(
                "DTSTART:20261007T090000Z\r\nDTEND:20261007T100000Z\r\nSUMMARY:Urgent meeting\r\n\
                 ORGANIZER:mailto:ceo@work.example\r\nATTENDEE;RSVP=TRUE:mailto:{MULTI_WORK_EMAIL}\r\n"
            ) }),
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
        &imip(&Imip { from: GRACE, to: MULTI_WORK_EMAIL, subject: "Accepted: Design review", message_id: "reply-1", method: "REPLY", uid: &event.uid, sequence: 0, stamp: "20261001T090000Z", body: &format!(
                "ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n"
            ) }),
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
        &imip(&Imip { from: GRACE, to: MULTI_WORK_EMAIL, subject: "New time proposed: Design review", message_id: "counter-1", method: "COUNTER", uid: &event.uid, sequence: 0, stamp: "20261001T100000Z", body: &format!(
                "DTSTART:20261008T150000Z\r\nDTEND:20261008T160000Z\r\nSUMMARY:Design review\r\n\
                 ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n\
                 COMMENT:Afternoon is better\r\n"
            ) }),
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

/// Delivers Grace's invitation `uid` and waits until it is in the calendar
/// (also proves that every message delivered before it was processed).
async fn invited(f: &Fixture, uid: &str, title: &str) -> (MessageSummary, Invitation) {
    let message = deliver(
        f,
        &invitation(uid, 0, title, "20261005T090000Z", "20261001T080000Z"),
        title,
    )
    .await;
    let card = until_some(&format!("{title:?} is in the calendar"), || async {
        let c = card(f, &message).await;
        c.event_id.is_some().then_some(c)
    })
    .await;
    (message, card)
}

/// A meeting the user organizes in the local calendar, with Grace invited.
async fn organized(f: &Fixture, title: &str) -> CalendarEvent {
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
        title,
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
    *event
}

async fn get_event(f: &Fixture, id: &nail_api::EventId) -> CalendarEvent {
    match ok(
        &f.service,
        Request::GetEvent {
            event_id: id.clone(),
        },
    )
    .await
    {
        Response::PimEvent { event } => *event,
        other => panic!("{other:?}"),
    }
}

const MALLORY: &str = "Mallory <mallory@evil.example>";

#[tokio::test(flavor = "multi_thread")]
async fn hijacks_by_uid_are_never_applied() {
    // Review M3b: anyone who knows a meeting's UID (a co-attendee, a
    // forwarded invitation) must not rewrite it.
    let f = setup().await;
    let uid = "probe-1@example.org";
    let (original, first) = invited(&f, uid, "Probe meeting").await;
    let event_id = first.event_id.clone().unwrap();

    // A: a REQUEST naming Mallory as the organizer, from Mallory.
    let hijack = deliver(
        &f,
        &imip(&Imip {
            from: MALLORY,
            to: MULTI_WORK_EMAIL,
            subject: "Hijacked",
            message_id: "hijack-a",
            method: "REQUEST",
            uid,
            sequence: 1,
            stamp: "20261002T080000Z",
            body: &format!(
                "DTSTART:20261005T090000Z\r\nDTEND:20261005T100000Z\r\nSUMMARY:Hijacked\r\n\
                 LOCATION:https://evil.example/join\r\nORGANIZER:mailto:mallory@evil.example\r\n\
                 ATTENDEE;RSVP=TRUE:mailto:{MULTI_WORK_EMAIL}\r\n"
            ),
        }),
        "Hijacked",
    )
    .await;
    // B: a PUBLISH over the event (no attendees, another organizer).
    let publish = deliver(
        &f,
        &imip(&Imip {
            from: MALLORY,
            to: MULTI_WORK_EMAIL,
            subject: "Published over",
            message_id: "hijack-b",
            method: "PUBLISH",
            uid,
            sequence: 0,
            stamp: "20261003T080000Z",
            body: "DTSTART:20261005T090000Z\r\nDTEND:20261005T100000Z\r\nSUMMARY:Published over\r\n\
             ORGANIZER:mailto:someone@evil.example\r\n",
        }),
        "Published over",
    )
    .await;
    // C: a new invitation "from the CEO", sent by Mallory with a SENT-BY
    // naming herself.
    let sent_by = deliver(
        &f,
        &imip(&Imip {
            from: MALLORY,
            to: MULTI_WORK_EMAIL,
            subject: "Board meeting",
            message_id: "hijack-c",
            method: "REQUEST",
            uid: "board-1@work.example",
            sequence: 0,
            stamp: "20261001T080000Z",
            body: &format!(
                "DTSTART:20261007T090000Z\r\nDTEND:20261007T100000Z\r\nSUMMARY:Board meeting\r\n\
                 ORGANIZER;SENT-BY=\"mailto:mallory@evil.example\":mailto:ceo@work.example\r\n\
                 ATTENDEE;RSVP=TRUE:mailto:{MULTI_WORK_EMAIL}\r\n"
            ),
        }),
        "Board meeting",
    )
    .await;
    // Everything delivered before this one has been processed.
    invited(&f, "sentinel-1@example.org", "Sentinel one").await;

    let copy = get_event(&f, &event_id).await;
    assert_eq!(copy.fields.title, "Probe meeting");
    assert_eq!(copy.fields.location.as_deref(), Some("Room 1"));
    assert_eq!(
        copy.fields.organizer.as_ref().map(|o| o.email.as_str()),
        Some("grace@example.org")
    );
    let a = card(&f, &hijack).await;
    assert!(a.issues.contains(&ItipIssue::OrganizerChanged), "{a:?}");
    let b = card(&f, &publish).await;
    assert!(b.issues.contains(&ItipIssue::OrganizerChanged), "{b:?}");
    let c = card(&f, &sent_by).await;
    assert!(c.issues.contains(&ItipIssue::SentOnBehalf), "{c:?}");
    assert_eq!(c.event_id, None);

    // Applying A by hand (the card still offers it, with the warning) keeps
    // the organizer, so the real organizer's later updates still apply on
    // their own.
    assert!(a.allows(InvitationAction::UpdateCalendar), "{a:?}");
    ok(
        &f.service,
        Request::ApplyInvitation {
            message_id: hijack.id.clone(),
            calendar_id: None,
        },
    )
    .await;
    let copy = get_event(&f, &event_id).await;
    assert_eq!(copy.fields.title, "Hijacked");
    assert_eq!(
        copy.fields.organizer.as_ref().map(|o| o.email.as_str()),
        Some("grace@example.org")
    );
    let update = deliver(
        &f,
        &invitation(
            uid,
            5,
            "Probe meeting (moved)",
            "20261006T090000Z",
            "20261004T080000Z",
        ),
        "Probe meeting (moved)",
    )
    .await;
    eventually("the real organizer's update is applied", || async {
        get_event(&f, &event_id).await.fields.title == "Probe meeting (moved)"
    })
    .await;
    assert_eq!(card(&f, &update).await.issues.len(), 0);
    assert_eq!(card(&f, &original).await.state, InvitationState::Outdated);
}

#[tokio::test(flavor = "multi_thread")]
async fn replies_from_uninvited_people_are_not_recorded() {
    // Review M3b: a REPLY from someone who is not invited would add them,
    // and every later REQUEST would go to them too.
    let f = setup().await;
    let event = organized(&f, "Closed meeting").await;
    let crasher = deliver(
        &f,
        &imip(&Imip { from: MALLORY, to: MULTI_WORK_EMAIL, subject: "Accepted: Closed meeting", message_id: "crasher-1", method: "REPLY", uid: &event.uid, sequence: 0, stamp: "20261001T090000Z", body: &format!(
                "ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:mallory@evil.example\r\n"
            ) }),
        "Accepted: Closed meeting",
    )
    .await;
    // Grace's own answer is recorded on arrival; Mallory's (delivered
    // first) has been processed by then.
    deliver(
        &f,
        &imip(&Imip { from: GRACE, to: MULTI_WORK_EMAIL, subject: "Accepted: Closed meeting (Grace)", message_id: "crasher-2", method: "REPLY", uid: &event.uid, sequence: 0, stamp: "20261001T100000Z", body: &format!(
                "ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n"
            ) }),
        "Accepted: Closed meeting (Grace)",
    )
    .await;
    eventually("Grace's answer is recorded", || async {
        get_event(&f, &event.id).await.fields.attendees[0].status == ParticipationStatus::Accepted
    })
    .await;
    let copy = get_event(&f, &event.id).await;
    let emails: Vec<&str> = copy
        .fields
        .attendees
        .iter()
        .map(|a| a.email.as_str())
        .collect();
    assert_eq!(emails, ["grace@example.org"]);
    let c = card(&f, &crasher).await;
    assert!(c.issues.contains(&ItipIssue::UninvitedAttendee), "{c:?}");
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
}

#[tokio::test(flavor = "multi_thread")]
async fn attendees_propose_refresh_and_forward() {
    let f = setup().await;
    let uid = "propose-1@example.org";
    let (message, first) = invited(&f, uid, "Proposal meeting").await;
    let target = InvitationTarget::Message {
        message_id: message.id.clone(),
    };

    // A new time: a COUNTER to the organizer.
    let result = scheduled(
        ok(
            &f.service,
            Request::ProposeNewTime {
                target: target.clone(),
                start: PimTime::utc("2026-10-05T15:00:00"),
                end: PimTime::utc("2026-10-05T16:00:00"),
                comment: Some("Afternoon?".into()),
            },
        )
        .await,
    );
    assert_eq!(result.sent.len(), 1);
    assert_eq!(result.sent[0].method, ItipMethod::Counter);
    assert_eq!(result.sent[0].recipients, ["grace@example.org"]);
    assert!(result.sent[0].outbox_id.is_some());
    eventually("the COUNTER reaches the SMTP server", || async {
        smtp(&f).iter().any(|m| {
            m.contains("METHOD:COUNTER")
                && m.contains(&format!("UID:{uid}"))
                && m.contains("DTSTART:20261005T150000Z")
                && m.contains("Afternoon?")
                && m.contains("grace@example.org")
        })
    })
    .await;
    // The calendar copy is unchanged until the organizer agrees.
    let copy = event(&f, &first).await.unwrap();
    assert_eq!(copy.fields.start, PimTime::utc("2026-10-05T09:00:00"));

    // Asking for the current version: a REFRESH, from the event itself.
    let result = scheduled(
        ok(
            &f.service,
            Request::RefreshInvitation {
                target: InvitationTarget::Event {
                    event_id: copy.id.clone(),
                    recurrence_id: None,
                },
            },
        )
        .await,
    );
    assert_eq!(result.sent[0].method, ItipMethod::Refresh);
    assert_eq!(result.sent[0].recipients, ["grace@example.org"]);
    eventually("the REFRESH reaches the SMTP server", || async {
        smtp(&f)
            .iter()
            .any(|m| m.contains("METHOD:REFRESH") && m.contains(&format!("UID:{uid}")))
    })
    .await;

    // Forwarding: the organizer's REQUEST, unchanged, to Dan.
    let result = scheduled(
        ok(
            &f.service,
            Request::ForwardInvitation {
                target,
                to: vec![nail_api::Address {
                    name: Some("Dan".into()),
                    email: "dan@example.org".into(),
                }],
                comment: Some("You should come".into()),
            },
        )
        .await,
    );
    assert_eq!(result.sent[0].method, ItipMethod::Request);
    assert_eq!(result.sent[0].recipients, ["dan@example.org"]);
    eventually("the forwarded REQUEST reaches the SMTP server", || async {
        smtp(&f).iter().any(|m| {
            m.contains("METHOD:REQUEST")
                && m.contains(&format!("UID:{uid}"))
                && m.contains("dan@example.org")
                && m.contains("mailto:grace@example.org")
        })
    })
    .await;
    // Bad recipients are refused.
    let response = f
        .service
        .handle(Request::ForwardInvitation {
            target: InvitationTarget::Message {
                message_id: message.id.clone(),
            },
            to: vec![nail_api::Address {
                name: None,
                email: "not an address".into(),
            }],
            comment: None,
        })
        .await;
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn declined_counters_tell_the_attendee_and_keep_the_time() {
    let f = setup().await;
    let event = organized(&f, "Fixed time review").await;
    let counter = deliver(
        &f,
        &imip(&Imip { from: GRACE, to: MULTI_WORK_EMAIL, subject: "New time proposed: Fixed time review", message_id: "decline-counter-1", method: "COUNTER", uid: &event.uid, sequence: 0, stamp: "20261001T100000Z", body: &format!(
                "DTSTART:20261008T150000Z\r\nDTEND:20261008T160000Z\r\nSUMMARY:Fixed time review\r\n\
                 ORGANIZER:mailto:{MULTI_WORK_EMAIL}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org\r\n"
            ) }),
        "New time proposed: Fixed time review",
    )
    .await;
    let c = card(&f, &counter).await;
    assert!(c.allows(InvitationAction::DeclineCounter), "{c:?}");
    let result = scheduled(
        ok(
            &f.service,
            Request::RespondToCounter {
                message_id: counter.id.clone(),
                accept: false,
                comment: Some("The room is booked".into()),
            },
        )
        .await,
    );
    assert_eq!(result.sent.len(), 1);
    assert_eq!(result.sent[0].method, ItipMethod::DeclineCounter);
    assert_eq!(result.sent[0].recipients, ["grace@example.org"]);
    eventually("the DECLINECOUNTER reaches the SMTP server", || async {
        smtp(&f).iter().any(|m| {
            m.contains("METHOD:DECLINECOUNTER")
                && m.contains(&format!("UID:{}", event.uid))
                && m.contains("The room is booked")
        })
    })
    .await;
    let copy = get_event(&f, &event.id).await;
    assert_eq!(copy.fields.start, PimTime::utc("2026-10-08T13:00:00"));
    assert_eq!(copy.sequence, event.sequence);
}

/// The message with `subject` in any folder of the work account.
async fn anywhere(f: &Fixture, subject: &str) -> Option<MessageSummary> {
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
    for folder in folders {
        if let Response::Messages { messages, .. } = ok(
            &f.service,
            Request::ListMessages {
                folder_id: folder.id,
                offset: 0,
                limit: 500,
            },
        )
        .await
            && let Some(m) = messages.into_iter().find(|m| m.subject == subject)
        {
            return Some(m);
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn junk_invitations_are_never_applied() {
    let f = setup().await;
    // The server marks it as junk ($Junk; honoured by default).
    let raw = invitation(
        "junk-1@example.org",
        0,
        "Cheap meeting",
        "20261005T090000Z",
        "20261001T080000Z",
    );
    f.demo
        .control("imap-deliver", &json!({ "raw": raw, "flags": ["$Junk"] }))
        .unwrap();
    let junk = until_some("the junk invitation arrives", || async {
        anywhere(&f, "Cheap meeting").await
    })
    .await;
    // A later invitation is added, so the junk one was processed.
    invited(&f, "sentinel-2@example.org", "Sentinel two").await;
    let c = card(&f, &junk).await;
    assert_eq!(c.event_id, None, "{c:?}");
    assert_eq!(c.state, InvitationState::New);
    assert!(c.allows(InvitationAction::AddToCalendar));
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_server_scheduling_never_delivers_twice() {
    // Review M3b: while a CalDAV server's scheduling is unknown (asking
    // failed), Nail mailed iMIP and the server, which schedules, delivered
    // too. Now the parties are marked SCHEDULE-AGENT=CLIENT on the server.
    let demo = start_demo_multi().await.unwrap();
    demo.dav.set_auto_schedule(true);
    // Every OPTIONS (only the scheduling discovery asks it) fails.
    for _ in 0..2000 {
        demo.dav.fail_next("OPTIONS", 500);
    }
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
        "Unknown scheduling",
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
    assert_eq!(result.sent[0].transport, SchedulingTransport::Imip);
    eventually("the REQUEST is mailed", || async {
        smtp(&f)
            .iter()
            .any(|m| m.contains("METHOD:REQUEST") && m.contains(&event.uid))
    })
    .await;
    // The copy on the server tells it to keep out.
    let stored = until_some("the event is on the CalDAV server", || async {
        f.demo.dav.collection_paths().into_iter().find_map(|path| {
            f.demo.dav.collection(&path).and_then(|c| {
                c.objects
                    .into_values()
                    .find(|o| o.data.contains(&event.uid))
                    .map(|o| o.data)
            })
        })
    })
    .await;
    let unfolded = stored.replace("\r\n ", "").replace("\r\n\t", "");
    assert!(
        unfolded.lines().any(|l| l.starts_with("ATTENDEE")
            && l.contains("SCHEDULE-AGENT=CLIENT")
            && l.contains(MULTI_DAV_OTHER_EMAIL)),
        "{stored}"
    );
    // Never both: the server delivered nothing for this event.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        !f.demo
            .dav
            .scheduled_messages()
            .iter()
            .any(|m| m.uid == event.uid),
        "{:?}",
        f.demo.dav.scheduled_messages()
    );
}
