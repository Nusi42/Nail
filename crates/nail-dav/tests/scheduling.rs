//! CalDAV scheduling (RFC 6638) and free/busy against the mock DAV server
//! of nail-testkit-net (loopback only): discovery of inbox / outbox /
//! addresses / auto-scheduling, free/busy through the outbox (also as the
//! contract's `FreeBusyProvider`), schedule-inbox processing, and the
//! auto-scheduling flow between two principals with `SCHEDULE-AGENT` /
//! `SCHEDULE-STATUS`.

use nail_api::ErrorCode;
use nail_api::engine::FreeBusyProvider;
use nail_api::ids::PimSourceId;
use nail_api::pim::calendar::AttendeeKind;
use nail_api::scheduling::{BusyKind, BusyPeriod, FreeBusyQuery, FreeBusySource, ItipMethod};
use nail_dav::scheduling::{post_free_busy, user_type};
use nail_dav::{
    CalDavFreeBusy, CollectionKind, DavAuth, DavClient, DavError, InboxDecision, ScheduleAgent,
    ScheduleDelivery, WriteOutcome, delete_inbox_item, delete_object, discover_scheduling,
    list_inbox, process_inbox, query_free_busy, schedule_statuses, server_schedules,
    set_schedule_agent, write_object,
};
use nail_testkit_net::{MockDavConfig, MockDavServer, MockDavUser};
use pretty_assertions::assert_eq;
use url::Url;

fn login(user: &str) -> DavClient {
    DavClient::new(DavAuth::Basic {
        username: user.into(),
        password: "secret".into(),
    })
    .unwrap()
}

async fn server(auto_schedule: bool) -> MockDavServer {
    MockDavServer::start(MockDavConfig {
        auto_schedule,
        users: vec![
            MockDavUser::new("bob"),
            MockDavUser::new("carol"),
            MockDavUser::room("room-1", "Board room"),
        ],
        ..MockDavConfig::default()
    })
    .await
    .unwrap()
}

fn url(server: &MockDavServer, path: &str) -> Url {
    Url::parse(&server.url(path)).unwrap()
}

fn event(uid: &str, start: &str, end: &str, extra: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail//Test//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20261001T000000Z\r\nDTSTART:{start}\r\nDTEND:{end}\r\n{extra}SUMMARY:{uid}\r\n\
         END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn period(start: &str, end: &str, kind: BusyKind) -> BusyPeriod {
    BusyPeriod {
        start: start.into(),
        end: end.into(),
        kind,
    }
}

fn query(attendees: &[&str]) -> FreeBusyQuery {
    FreeBusyQuery {
        organizer: "ada@example.org".into(),
        attendees: attendees.iter().map(|a| (*a).to_owned()).collect(),
        from: "2026-10-05T00:00:00Z".into(),
        to: "2026-10-06T00:00:00Z".into(),
    }
}

// ---------------------------------------------------------------------------
// Discovery

#[tokio::test]
async fn discovers_inbox_outbox_addresses_and_capabilities() {
    let server = server(false).await;
    let principal = url(&server, &server.principal_path());
    let home = url(&server, &server.calendar_home());
    let info = discover_scheduling(&login("ada"), &principal, Some(&home))
        .await
        .unwrap();
    assert_eq!(
        info.inbox.as_ref().unwrap().path(),
        server.inbox_path("ada")
    );
    assert_eq!(
        info.outbox.as_ref().unwrap().path(),
        server.outbox_path("ada")
    );
    assert_eq!(info.addresses, ["ada@example.org"]);
    assert!(info.is_own_address("MAILTO:Ada@example.org"));
    assert_eq!(info.user_type, AttendeeKind::Individual);
    assert!(!info.auto_schedule);
    assert!(info.supports_free_busy());
    assert_eq!(
        info.default_calendar.as_ref().unwrap().path(),
        server.calendar_path("personal")
    );

    server.set_auto_schedule(true);
    let info = discover_scheduling(&login("ada"), &principal, None)
        .await
        .unwrap();
    assert!(info.auto_schedule);

    // Another principal, seen with its own login: a room.
    let room = url(&server, &server.user_principal_path("room-1"));
    let info = discover_scheduling(&login("room-1"), &room, None)
        .await
        .unwrap();
    assert_eq!(info.user_type, AttendeeKind::Room);
    assert_eq!(info.display_name.as_deref(), Some("Board room"));
    assert_eq!(info.addresses, ["room-1@example.org"]);
    assert_eq!(user_type("ROOM"), AttendeeKind::Room);
}

#[tokio::test]
async fn servers_without_scheduling_say_so() {
    let server = MockDavServer::start(MockDavConfig {
        scheduling: false,
        ..MockDavConfig::default()
    })
    .await
    .unwrap();
    let principal = url(&server, &server.principal_path());
    let err = discover_scheduling(&login("ada"), &principal, None)
        .await
        .unwrap_err();
    assert_eq!(err, DavError::NoScheduling);
    assert_eq!(err.code(), ErrorCode::Unsupported);
    assert_eq!(err.localized().id, "dav-no-scheduling");
    let refused = discover_scheduling(&login("mallory"), &principal, None)
        .await
        .unwrap_err();
    assert_eq!(refused, DavError::AuthFailed);
}

// ---------------------------------------------------------------------------
// Free/busy

fn seed_bob(server: &MockDavServer) {
    let personal = server.user_calendar_path("bob", "personal");
    let put = |name: &str, data: String| {
        server.put_object(&personal, name, &data).unwrap();
    };
    put(
        "busy.ics",
        event("busy", "20261005T090000Z", "20261005T100000Z", ""),
    );
    put(
        "overlap.ics",
        event("overlap", "20261005T093000Z", "20261005T103000Z", ""),
    );
    put(
        "tentative.ics",
        event(
            "tentative",
            "20261005T120000Z",
            "20261005T130000Z",
            "STATUS:TENTATIVE\r\n",
        ),
    );
    put(
        "free.ics",
        event(
            "free",
            "20261005T140000Z",
            "20261005T150000Z",
            "TRANSP:TRANSPARENT\r\n",
        ),
    );
    put(
        "cancelled.ics",
        event(
            "cancelled",
            "20261005T150000Z",
            "20261005T160000Z",
            "STATUS:CANCELLED\r\n",
        ),
    );
    put(
        "declined.ics",
        event(
            "declined",
            "20261005T160000Z",
            "20261005T170000Z",
            "ORGANIZER:mailto:dan@example.org\r\nATTENDEE;PARTSTAT=DECLINED:mailto:bob@example.org\r\n",
        ),
    );
    put(
        "late.ics",
        event("late", "20261005T230000Z", "20261006T010000Z", ""),
    );
    put(
        "other-day.ics",
        event("other-day", "20261007T090000Z", "20261007T100000Z", ""),
    );
}

#[tokio::test]
async fn free_busy_through_the_outbox() {
    let server = server(false).await;
    seed_bob(&server);
    server.set_free_busy_visible("carol", false);
    let outbox = url(&server, &server.outbox_path("ada"));
    let source = PimSourceId::new("work-caldav");
    let answers = query_free_busy(
        &login("ada"),
        &outbox,
        &query(&["BOB@example.org", "nobody@example.org", "carol@example.org"]),
        &source,
    )
    .await
    .unwrap();
    assert_eq!(answers.len(), 2, "the unknown address is left out");
    assert_eq!(answers[0].email, "BOB@example.org");
    assert_eq!(
        answers[0].source,
        FreeBusySource::Caldav {
            source_id: source.clone()
        }
    );
    assert_eq!(answers[0].error, None);
    assert_eq!(
        answers[0].periods,
        vec![
            period(
                "2026-10-05T09:00:00Z",
                "2026-10-05T10:30:00Z",
                BusyKind::Busy
            ),
            period(
                "2026-10-05T12:00:00Z",
                "2026-10-05T13:00:00Z",
                BusyKind::Tentative
            ),
            period(
                "2026-10-05T23:00:00Z",
                "2026-10-06T00:00:00Z",
                BusyKind::Busy
            ),
        ]
    );
    assert_eq!(answers[1].email, "carol@example.org");
    assert_eq!(answers[1].error, Some(ErrorCode::Forbidden));
    assert!(answers[1].periods.is_empty());
    assert_eq!(
        answers[1].localized.as_ref().unwrap().id,
        "dav-freebusy-no-access"
    );

    let posts: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].path, server.outbox_path("ada"));
    assert!(
        posts[0]
            .content_type
            .as_deref()
            .unwrap()
            .starts_with("text/calendar")
    );
    assert!(posts[0].body.contains("BEGIN:VFREEBUSY"));
    assert!(posts[0].body.contains("ORGANIZER:mailto:ada@example.org"));
}

#[tokio::test]
async fn free_busy_provider_batches_validates_and_fixes_the_organizer() {
    let server = server(false).await;
    seed_bob(&server);
    let principal = url(&server, &server.principal_path());
    let info = discover_scheduling(&login("ada"), &principal, None)
        .await
        .unwrap();
    let provider = CalDavFreeBusy::new(login("ada"), PimSourceId::new("s"), info.clone()).unwrap();
    assert_eq!(provider.info(), &info);

    // A foreign organizer is replaced by the principal's address (the
    // server refuses others), and 30 attendees go out in two POSTs.
    let mut many: Vec<String> = (0..29).map(|i| format!("x{i}@example.org")).collect();
    many.push("bob@example.org".into());
    let mut q = query(&[]);
    q.attendees = many;
    q.organizer = "ada.private@example.net".into();
    assert_eq!(provider.effective_query(&q).organizer, "ada@example.org");
    let answers = provider.free_busy(&q).await.unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].email, "bob@example.org");
    assert_eq!(answers[0].periods.len(), 3);
    let posts = server
        .requests()
        .iter()
        .filter(|r| r.method == "POST")
        .count();
    assert_eq!(posts, 2);

    // Invalid questions never reach the server.
    let bad = FreeBusyQuery {
        to: "2026-10-04T00:00:00Z".into(),
        ..query(&["bob@example.org"])
    };
    let err = provider.free_busy(&bad).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert_eq!(err.localized.unwrap().id, "freebusy-issue-invalid-range");
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        2
    );

    // Without an outbox there is no provider.
    let mut no_outbox = info;
    no_outbox.outbox = None;
    assert!(CalDavFreeBusy::new(login("ada"), PimSourceId::new("s"), no_outbox).is_none());
}

#[tokio::test]
async fn outbox_refusals_are_errors() {
    let server = server(false).await;
    let outbox = url(&server, &server.outbox_path("ada"));
    // The server refuses an organizer that is not the user.
    let mut q = query(&["bob@example.org"]);
    q.organizer = "mallory@example.org".into();
    let err = post_free_busy(&login("ada"), &outbox, &q)
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Forbidden(_)), "{err:?}");
    // Someone else's outbox.
    let err = post_free_busy(&login("bob"), &outbox, &query(&["carol@example.org"]))
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Forbidden(_)), "{err:?}");
    // A server failure is retryable.
    server.fail_next("POST", 503);
    let err = post_free_busy(&login("ada"), &outbox, &query(&["bob@example.org"]))
        .await
        .unwrap_err();
    assert_eq!(err, DavError::RateLimited(Some(1)));
    assert!(err.retryable());
    // A range that is not RFC 3339 is not sent.
    let mut q = query(&["bob@example.org"]);
    q.from = "monday".into();
    assert!(matches!(
        post_free_busy(&login("ada"), &outbox, &q).await,
        Err(DavError::Protocol(_))
    ));
}

// ---------------------------------------------------------------------------
// The schedule inbox

fn itip(method: &str, uid: &str, stamp: &str, from_attendee: bool) -> String {
    let (organizer, attendee) = if from_attendee {
        ("ada@example.org", "bob@example.org")
    } else {
        ("bob@example.org", "ada@example.org")
    };
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nMETHOD:{method}\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:{stamp}\r\nDTSTART:20261005T090000Z\r\nDTEND:20261005T100000Z\r\n\
         ORGANIZER:mailto:{organizer}\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:{attendee}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

#[tokio::test]
async fn schedule_inbox_items_are_read_processed_and_deleted() {
    let server = server(false).await;
    let inbox = url(&server, &server.inbox_path("ada"));
    let client = login("ada");
    assert!(list_inbox(&client, &inbox).await.unwrap().is_empty());
    server
        .deliver(
            "ada",
            "b.ics",
            &itip("REPLY", "m-2", "20261002T000000Z", true),
        )
        .unwrap();
    server
        .deliver(
            "ada",
            "a.ics",
            &itip("REQUEST", "m-1", "20261003T000000Z", false),
        )
        .unwrap();
    server
        .deliver(
            "ada",
            "c.ics",
            &itip("CANCEL", "m-1", "20261004T000000Z", false),
        )
        .unwrap();
    let items = list_inbox(&client, &inbox).await.unwrap();
    let order: Vec<(Option<ItipMethod>, Option<&str>)> = items
        .iter()
        .map(|i| (i.method, i.sender.as_deref()))
        .collect();
    assert_eq!(
        order,
        [
            (Some(ItipMethod::Reply), Some("bob@example.org")),
            (Some(ItipMethod::Request), Some("bob@example.org")),
            (Some(ItipMethod::Cancel), Some("bob@example.org")),
        ],
        "oldest DTSTAMP first; replies come from the attendee"
    );
    assert!(items.iter().all(|i| i.etag.is_some()));
    assert_eq!(items[1].uid.as_deref(), Some("m-1"));

    // The caller applies what it can and keeps the rest.
    let mut seen = Vec::new();
    let report = process_inbox(&client, &inbox, async |item| {
        seen.push(item.href.clone());
        if item.method == Some(ItipMethod::Reply) {
            InboxDecision::Keep
        } else {
            InboxDecision::Delete
        }
    })
    .await
    .unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(report.deleted.len(), 2);
    assert_eq!(report.kept, [format!("{}b.ics", server.inbox_path("ada"))]);
    let left: Vec<String> = server.inbox("ada").into_iter().map(|(n, _)| n).collect();
    assert_eq!(left, ["b.ics"]);

    // Deleting an item that is already gone, or changed since, is fine.
    let item = list_inbox(&client, &inbox).await.unwrap().remove(0);
    server
        .deliver(
            "ada",
            "b.ics",
            &itip("REPLY", "m-2", "20261005T000000Z", true),
        )
        .unwrap();
    delete_inbox_item(&client, &item).await.unwrap();
    assert_eq!(server.inbox("ada").len(), 1, "a changed item stays");
    server.delete_object(&server.inbox_path("ada"), "b.ics");
    delete_inbox_item(&client, &item).await.unwrap();

    // Another user's inbox is not writable by PUT.
    let err = login("ada")
        .put(
            &url(&server, &format!("{}x.ics", server.inbox_path("ada"))),
            CollectionKind::Calendar,
            &itip("REQUEST", "x", "20261001T000000Z", false),
            &nail_dav::Precondition::None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DavError::Forbidden(_)));
}

// ---------------------------------------------------------------------------
// Auto-scheduling between two principals

fn meeting(extra_attendees: &str) -> String {
    event(
        "meet-1",
        "20261005T140000Z",
        "20261005T150000Z",
        &format!(
            "SEQUENCE:0\r\nORGANIZER;CN=Ada:mailto:ada@example.org\r\n\
             ATTENDEE;PARTSTAT=ACCEPTED:mailto:ada@example.org\r\n\
             ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:bob@example.org\r\n{extra_attendees}"
        ),
    )
}

#[tokio::test]
async fn auto_scheduling_delivers_requests_replies_and_cancels() {
    let server = server(true).await;
    let ada = login("ada");
    let bob = login("bob");
    let ada_principal = url(&server, &server.principal_path());
    let ada_info = discover_scheduling(&ada, &ada_principal, None)
        .await
        .unwrap();
    assert!(ada_info.auto_schedule);

    // Ada organizes: the server delivers to Bob (local), "mails" the
    // external attendee and leaves Carol (SCHEDULE-AGENT=CLIENT) to Nail.
    let text = meeting(
        "ATTENDEE:mailto:ext@other.example\r\nATTENDEE;SCHEDULE-AGENT=CLIENT:mailto:carol@example.org\r\n",
    );
    assert!(server_schedules(
        &text,
        &ada_info.addresses,
        ada_info.auto_schedule
    ));
    let ada_url = url(
        &server,
        &format!("{}meet-1.ics", server.calendar_path("personal")),
    );
    let WriteOutcome::Written { etag, data } =
        write_object(&ada, &ada_url, CollectionKind::Calendar, &text, None)
            .await
            .unwrap()
    else {
        panic!("conflict");
    };
    assert!(etag.is_some(), "read back after the server changed it");
    let stored = data.expect("the server's version was read back");
    let statuses = schedule_statuses(&stored);
    let state = |email: &str| {
        statuses
            .iter()
            .find(|s| s.email == email)
            .map(|s| (s.agent, s.delivery()))
    };
    assert_eq!(
        state("bob@example.org"),
        Some((ScheduleAgent::Server, Some(ScheduleDelivery::Delivered)))
    );
    assert_eq!(
        state("ext@other.example"),
        Some((ScheduleAgent::Server, Some(ScheduleDelivery::Pending)))
    );
    assert_eq!(
        state("carol@example.org"),
        Some((ScheduleAgent::Client, None))
    );
    let sent: Vec<(String, String, bool)> = server
        .scheduled_messages()
        .into_iter()
        .map(|m| (m.method, m.to, m.local))
        .collect();
    assert_eq!(
        sent,
        [
            ("REQUEST".into(), "bob@example.org".into(), true),
            ("REQUEST".into(), "ext@other.example".into(), false),
        ]
    );
    assert!(server.inbox("carol").is_empty());

    // Bob finds the invitation in his inbox and his calendar.
    let bob_inbox = url(&server, &server.inbox_path("bob"));
    let items = list_inbox(&bob, &bob_inbox).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].method, Some(ItipMethod::Request));
    assert_eq!(items[0].sender.as_deref(), Some("ada@example.org"));
    assert!(!items[0].data.contains("SCHEDULE-STATUS"));
    let bob_personal = server.user_calendar_path("bob", "personal");
    let copy = server.object(&bob_personal, "meet-1.ics").unwrap();
    delete_inbox_item(&bob, &items[0]).await.unwrap();

    // Bob accepts by writing his copy: the server replies to Ada.
    let bob_addresses = vec!["bob@example.org".to_owned()];
    assert!(server_schedules(&copy.data, &bob_addresses, true));
    let accepted = copy.data.replace(
        "PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:bob@example.org",
        "PARTSTAT=ACCEPTED:mailto:bob@example.org",
    );
    let bob_url = url(&server, &format!("{bob_personal}meet-1.ics"));
    let WriteOutcome::Written { data, .. } = write_object(
        &bob,
        &bob_url,
        CollectionKind::Calendar,
        &accepted,
        Some(&copy.etag),
    )
    .await
    .unwrap() else {
        panic!("conflict");
    };
    assert!(
        data.unwrap()
            .contains("SCHEDULE-STATUS=1.2:mailto:ada@example.org")
    );
    let ada_inbox = url(&server, &server.inbox_path("ada"));
    let replies = list_inbox(&ada, &ada_inbox).await.unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].method, Some(ItipMethod::Reply));
    assert_eq!(replies[0].sender.as_deref(), Some("bob@example.org"));
    assert!(!replies[0].data.contains("ext@other.example"));
    let organizer_copy = server
        .object(&server.calendar_path("personal"), "meet-1.ics")
        .unwrap();
    assert!(
        organizer_copy.data.contains(
            "ATTENDEE;PARTSTAT=ACCEPTED;RSVP=TRUE;SCHEDULE-STATUS=1.2:mailto:bob@example.org"
        ),
        "{}",
        organizer_copy.data
    );
    let bob_state = schedule_statuses(&organizer_copy.data)
        .into_iter()
        .find(|s| s.email == "bob@example.org")
        .unwrap();
    assert_eq!(bob_state.delivery(), Some(ScheduleDelivery::Delivered));

    // Free/busy now shows the meeting for Bob.
    let outbox = url(&server, &server.outbox_path("ada"));
    let fb = query_free_busy(
        &ada,
        &outbox,
        &query(&["bob@example.org"]),
        &PimSourceId::new("s"),
    )
    .await
    .unwrap();
    assert_eq!(
        fb[0].periods,
        [period(
            "2026-10-05T14:00:00Z",
            "2026-10-05T15:00:00Z",
            BusyKind::Busy
        )]
    );

    // Ada cancels by deleting: Bob gets a CANCEL and his copy is cancelled.
    delete_object(&ada, &ada_url, None).await.unwrap();
    let bob_items = list_inbox(&bob, &bob_inbox).await.unwrap();
    assert_eq!(bob_items.len(), 1);
    assert_eq!(bob_items[0].method, Some(ItipMethod::Cancel));
    let cancelled = server.object(&bob_personal, "meet-1.ics").unwrap();
    assert!(cancelled.data.contains("STATUS:CANCELLED"));
    let fb = query_free_busy(
        &ada,
        &outbox,
        &query(&["bob@example.org"]),
        &PimSourceId::new("s"),
    )
    .await
    .unwrap();
    assert!(fb[0].periods.is_empty(), "a cancelled meeting is free");
}

#[tokio::test]
async fn client_scheduling_and_servers_that_do_not_schedule() {
    // SCHEDULE-AGENT=CLIENT everywhere: the server delivers nothing and
    // answers with the ETag of exactly what was sent.
    let server = server(true).await;
    let ada = login("ada");
    let text = set_schedule_agent(&meeting(""), ScheduleAgent::Client);
    assert!(!server_schedules(
        &text,
        &["ada@example.org".to_owned()],
        true
    ));
    let target = url(
        &server,
        &format!("{}meet-1.ics", server.calendar_path("personal")),
    );
    let outcome = write_object(&ada, &target, CollectionKind::Calendar, &text, None)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        WriteOutcome::Written {
            etag: Some(_),
            data: None
        }
    ));
    assert!(server.scheduled_messages().is_empty());
    assert!(server.inbox("bob").is_empty());

    // With auto-scheduling off, plain meetings are not delivered either.
    server.set_auto_schedule(false);
    assert!(!server.auto_schedule());
    let other = url(
        &server,
        &format!("{}meet-2.ics", server.calendar_path("personal")),
    );
    let plain = meeting("").replace("meet-1", "meet-2");
    let outcome = write_object(&ada, &other, CollectionKind::Calendar, &plain, None)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        WriteOutcome::Written {
            etag: Some(_),
            data: None
        }
    ));
    assert!(server.scheduled_messages().is_empty());
    let info = discover_scheduling(&ada, &url(&server, &server.principal_path()), None)
        .await
        .unwrap();
    assert!(!info.auto_schedule);
    assert!(!server_schedules(
        &plain,
        &info.addresses,
        info.auto_schedule
    ));
}

#[tokio::test]
async fn attendee_declines_by_deleting() {
    let server = server(true).await;
    let ada = login("ada");
    let target = url(
        &server,
        &format!("{}meet-1.ics", server.calendar_path("personal")),
    );
    write_object(&ada, &target, CollectionKind::Calendar, &meeting(""), None)
        .await
        .unwrap();
    let bob_personal = server.user_calendar_path("bob", "personal");
    let bob_copy = url(&server, &format!("{bob_personal}meet-1.ics"));
    delete_object(&login("bob"), &bob_copy, None).await.unwrap();
    let last = server.scheduled_messages().pop().unwrap();
    assert_eq!(last.method, "REPLY");
    assert_eq!(last.to, "ada@example.org");
    assert!(last.data.contains("PARTSTAT=DECLINED"));
    let organizer_copy = server
        .object(&server.calendar_path("personal"), "meet-1.ics")
        .unwrap();
    assert!(organizer_copy.data.contains("PARTSTAT=DECLINED"));
}

#[tokio::test]
async fn a_failing_options_request_leaves_scheduling_unknown() {
    // Review M3b: a transient OPTIONS failure used to be stored as "the
    // server does not schedule", so Nail mailed what the server delivered.
    let server = server(true).await;
    let principal = url(&server, &server.principal_path());
    server.fail_next("OPTIONS", 500);
    let err = discover_scheduling(&login("ada"), &principal, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, DavError::Status { status: 500, .. }),
        "{err:?}"
    );
    // A server that does not know OPTIONS does not schedule.
    server.fail_next("OPTIONS", 405);
    let info = discover_scheduling(&login("ada"), &principal, None)
        .await
        .unwrap();
    assert!(!info.auto_schedule);
    // Asked again: it schedules.
    let info = discover_scheduling(&login("ada"), &principal, None)
        .await
        .unwrap();
    assert!(info.auto_schedule);
}
