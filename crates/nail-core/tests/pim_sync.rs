//! M3a integration (ADR 0063): the PIM source engines against in-process
//! mock servers — CalDAV / CardDAV both ways (`MockDavServer`), a
//! read-only ICS subscription (`MockHttpServer`) — and reminders firing
//! at the controlled clock time.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::pim::{
    Alarm, ContactFields, EventFields, EventInstance, LabeledValue, PimSourceState, PimTime,
};
use nail_api::triage::NotificationKind;
use nail_api::{CalendarId, ErrorCode, Event, Request, Response, SecretRef};
use nail_core::{AdjustableClock, EngineTiming, Service, ServiceOptions};
use nail_testkit_net::{MockDavConfig, MockDavServer, MockHttpServer, MockResponse};

struct Fixture {
    service: Service,
    _data: tempfile::TempDir,
    _config: tempfile::TempDir,
    clock: Arc<AdjustableClock>,
}

async fn start(accounts_kdl: &str, secrets: &[(&str, &str)]) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(config.path().join("accounts.kdl"), accounts_kdl).unwrap();
    let store = Arc::new(MemorySecretStore::new());
    for (reference, value) in secrets {
        store
            .set(&SecretRef((*reference).to_owned()), value)
            .unwrap();
    }
    let clock = Arc::new(AdjustableClock::new());
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = true;
    options.secrets = Some(store as Arc<dyn SecretStore>);
    let mut timing = EngineTiming::fast();
    timing.poll_override = Some(Duration::from_secs(30));
    options.timing = timing;
    options.clock = Some(Arc::clone(&clock) as Arc<dyn nail_triage::Clock>);
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    Fixture {
        service,
        _data: data,
        _config: config,
        clock,
    }
}

async fn ok(service: &Service, request: Request) -> Response {
    match service.handle(request).await {
        Response::Error { code, message, .. } => panic!("{code:?}: {message}"),
        other => other,
    }
}

/// Polls `check` until it returns `Some` (at most 20 s).
async fn eventually<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    for _ in 0..200 {
        if let Some(found) = check().await {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn calendars(service: &Service, source: &str) -> Vec<nail_api::pim::Calendar> {
    match ok(
        service,
        Request::ListCalendars {
            source_id: Some(source.into()),
        },
    )
    .await
    {
        Response::Calendars { calendars } => calendars,
        other => panic!("{other:?}"),
    }
}

async fn instances(service: &Service, calendar: &CalendarId) -> Vec<EventInstance> {
    match ok(
        service,
        Request::ListEventInstances {
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-11-01T00:00:00Z".into(),
            calendar_ids: Some(vec![calendar.clone()]),
            tz: "UTC".into(),
        },
    )
    .await
    {
        Response::EventInstances { instances, .. } => instances,
        other => panic!("{other:?}"),
    }
}

const SERVER_EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\n\
BEGIN:VEVENT\r\nUID:server-1@other\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261008T140000Z\r\n\
DTEND:20261008T150000Z\r\nSUMMARY:From another client\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

#[tokio::test(flavor = "multi_thread")]
async fn caldav_and_carddav_sync_both_ways() {
    let dav = MockDavServer::start(MockDavConfig::default())
        .await
        .unwrap();
    let url = format!("{}/", dav.base_url());
    let kdl = format!(
        "caldav \"dav\" url=\"{url}\" {{\n    auth \"password\" username=\"ada\"\n}}\n\
carddav \"cards\" url=\"{url}\" {{\n    auth \"password\" username=\"ada\"\n}}\n"
    );
    let f = start(
        &kdl,
        &[
            ("keychain:nail/pim/dav/password", "secret"),
            ("keychain:nail/pim/cards/password", "secret"),
        ],
    )
    .await;
    let service = &f.service;
    // Discovery found the seeded calendars (the holidays one read-only).
    let found = eventually("the DAV calendars", || async {
        let list = calendars(service, "dav").await;
        (list.len() >= 3).then_some(list)
    })
    .await;
    let personal = found
        .iter()
        .find(|c| c.settings.name == "Personal")
        .unwrap()
        .clone();
    assert!(
        found
            .iter()
            .any(|c| c.settings.name == "Holidays" && c.read_only)
    );
    // An event created here is uploaded.
    let Response::EventSaved { event, op_ids, .. } = ok(
        service,
        Request::CreateEvent {
            calendar_id: personal.id.clone(),
            fields: Box::new(EventFields::new(
                "Uploaded",
                PimTime::utc("2026-10-07T09:00:00"),
                PimTime::utc("2026-10-07T10:00:00"),
            )),
            recurrence: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(op_ids.len(), 1);
    let path = dav.calendar_path("personal");
    eventually("the upload", || async {
        let collection = dav.collection(&path)?;
        collection
            .objects
            .values()
            .any(|o| o.data.contains("SUMMARY:Uploaded"))
            .then_some(())
    })
    .await;
    eventually("the synced state", || async {
        let Response::PimEvent { event: e } = ok(
            service,
            Request::GetEvent {
                event_id: event.id.clone(),
            },
        )
        .await
        else {
            return None;
        };
        (e.sync == nail_api::pim::PimItemSync::Synced).then_some(())
    })
    .await;
    // Another client's change shows after the next sync.
    dav.put_object(&path, "server-1.ics", SERVER_EVENT).unwrap();
    ok(service, Request::SyncPimSources { source_id: None }).await;
    eventually("the server's event", || async {
        instances(service, &personal.id)
            .await
            .iter()
            .any(|i| i.title == "From another client")
            .then_some(())
    })
    .await;
    // The read-only calendar refuses writes.
    let holidays = found.iter().find(|c| c.read_only).unwrap();
    match service
        .handle(Request::CreateEvent {
            calendar_id: holidays.id.clone(),
            fields: Box::new(EventFields::new(
                "No",
                PimTime::date("2026-10-09"),
                PimTime::date("2026-10-10"),
            )),
            recurrence: None,
        })
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    // CardDAV: a contact is uploaded as vCard.
    let book = eventually("the address book", || async {
        let Response::AddressBooks { address_books } = ok(
            service,
            Request::ListAddressBooks {
                source_id: Some("cards".into()),
            },
        )
        .await
        else {
            return None;
        };
        address_books.into_iter().next()
    })
    .await;
    let mut fields = ContactFields::named("Grace Hopper");
    fields.emails.push(LabeledValue::new("grace@navy.example"));
    ok(
        service,
        Request::CreateContact {
            address_book_id: book.id.clone(),
            fields: Box::new(fields),
        },
    )
    .await;
    let book_path = dav.address_book_path("contacts");
    eventually("the vCard upload", || async {
        dav.collection(&book_path)?
            .objects
            .values()
            .any(|o| o.data.contains("FN:Grace Hopper"))
            .then_some(())
    })
    .await;
    // A deleted event goes from the server too.
    ok(
        service,
        Request::DeleteEvent {
            event_id: event.id.clone(),
            scope: nail_api::pim::EditScope::All,
            recurrence_id: None,
        },
    )
    .await;
    eventually("the server delete", || async {
        (!dav
            .collection(&path)?
            .objects
            .values()
            .any(|o| o.data.contains("SUMMARY:Uploaded")))
        .then_some(())
    })
    .await;
    let Response::PimSources { sources } = ok(service, Request::ListPimSources).await else {
        panic!()
    };
    let dav_source = sources.iter().find(|s| s.id.as_str() == "dav").unwrap();
    assert!(
        matches!(
            dav_source.state,
            PimSourceState::Idle | PimSourceState::Syncing
        ),
        "{dav_source:?}"
    );
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_password_is_an_auth_error() {
    let dav = MockDavServer::start(MockDavConfig::default())
        .await
        .unwrap();
    let kdl = format!(
        "caldav \"dav\" url=\"{}/\" {{\n    auth \"password\" username=\"ada\"\n}}\n",
        dav.base_url()
    );
    let f = start(&kdl, &[("keychain:nail/pim/dav/password", "wrong")]).await;
    let service = &f.service;
    eventually("the auth error", || async {
        let Response::PimSources { sources } = ok(service, Request::ListPimSources).await else {
            return None;
        };
        let source = sources.into_iter().find(|s| s.id.as_str() == "dav")?;
        (source.state == PimSourceState::AuthError).then_some(source)
    })
    .await;
    service.shutdown().await;
}

const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Town//EN\r\nX-WR-CALNAME:Town holidays\r\n\
BEGIN:VEVENT\r\nUID:h1@town\r\nDTSTAMP:20261001T000000Z\r\nDTSTART;VALUE=DATE:20261003\r\n\
DTEND;VALUE=DATE:20261004\r\nSUMMARY:Unity Day\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

#[tokio::test(flavor = "multi_thread")]
async fn ics_subscriptions_are_read_only() {
    let http = MockHttpServer::start().await.unwrap();
    http.get(
        None,
        "/holidays.ics",
        MockResponse {
            status: 200,
            content_type: "text/calendar".into(),
            body: FEED.as_bytes().to_vec(),
            headers: vec![("ETag".into(), "\"v1\"".into())],
            delay: None,
        },
    );
    let kdl = format!(
        "calendar-subscription \"town\" url=\"{}/holidays.ics\" color=\"#cc3333\"\n",
        http.base_url()
    );
    let f = start(&kdl, &[]).await;
    let service = &f.service;
    let calendar = eventually("the feed's calendar", || async {
        calendars(service, "town").await.into_iter().next()
    })
    .await;
    assert!(calendar.read_only);
    assert_eq!(calendar.settings.color.as_deref(), Some("#cc3333"));
    let list = eventually("the feed's events", || async {
        let list = instances(service, &calendar.id).await;
        (!list.is_empty()).then_some(list)
    })
    .await;
    assert_eq!(list[0].title, "Unity Day");
    assert!(list[0].all_day && list[0].read_only);
    match service
        .handle(Request::CreateEvent {
            calendar_id: calendar.id.clone(),
            fields: Box::new(EventFields::new(
                "Mine",
                PimTime::date("2026-10-09"),
                PimTime::date("2026-10-10"),
            )),
            recurrence: None,
        })
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    // The feed's ETag is kept for the next, conditional fetch.
    let sync = service
        .store()
        .collection_sync(calendar.id.as_str())
        .unwrap();
    assert_eq!(sync.etag.as_deref(), Some("\"v1\""));
    ok(service, Request::SyncPimSources { source_id: None }).await;
    eventually("a second fetch", || async {
        (http.requests().len() >= 2).then_some(())
    })
    .await;
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reminders_fire_at_the_clock_time() {
    let f = start("", &[]).await;
    let service = &f.service;
    let start_at = chrono::DateTime::parse_from_rfc3339("2026-10-05T09:00:00Z")
        .unwrap()
        .timestamp();
    f.clock.set(start_at - 3600);
    let Response::Calendars { calendars } =
        ok(service, Request::ListCalendars { source_id: None }).await
    else {
        panic!()
    };
    let calendar = calendars
        .iter()
        .find(|c| c.has_events())
        .unwrap()
        .id
        .clone();
    let mut events = service.subscribe();
    let mut fields = EventFields::new(
        "Dentist",
        PimTime::utc("2026-10-05T09:00:00"),
        PimTime::utc("2026-10-05T09:30:00"),
    );
    fields.location = Some("Main street 1".into());
    fields.alarms.push(Alarm::minutes_before(15));
    ok(
        service,
        Request::CreateEvent {
            calendar_id: calendar,
            fields: Box::new(fields),
            recurrence: None,
        },
    )
    .await;
    // Nothing yet at 08:00.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let Response::Reminders { reminders } =
        ok(service, Request::ListReminders { until: None }).await
    else {
        panic!()
    };
    assert!(reminders.is_empty(), "{reminders:?}");
    // At 08:45 it fires: ReminderDue and a notification.
    f.clock.set(start_at - 15 * 60);
    let (mut due, mut notified) = (None, None);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while (due.is_none() || notified.is_none()) && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            Ok(Ok(Event::ReminderDue { reminder })) => due = Some(reminder),
            Ok(Ok(Event::NotificationRequested { notification }))
                if notification.kind == NotificationKind::Reminder =>
            {
                notified = Some(notification);
            }
            _ => {}
        }
    }
    let due = due.expect("ReminderDue");
    assert_eq!(due.title, "Dentist");
    assert_eq!(due.fire_at, "2026-10-05T08:45:00Z");
    let notification = notified.expect("a reminder notification");
    assert_eq!(notification.id, format!("reminder:{}", due.id));
    let Response::Reminders { reminders } =
        ok(service, Request::ListReminders { until: None }).await
    else {
        panic!()
    };
    assert_eq!(reminders.len(), 1);
    // Snooze past the clock, then dismiss: the alarm is acknowledged.
    ok(
        service,
        Request::SnoozeReminders {
            reminder_ids: vec![due.id.clone()],
            until: "2026-10-05T08:55:00Z".into(),
        },
    )
    .await;
    ok(
        service,
        Request::DismissReminders {
            reminder_ids: vec![due.id.clone()],
        },
    )
    .await;
    let Response::Reminders { reminders } =
        ok(service, Request::ListReminders { until: None }).await
    else {
        panic!()
    };
    assert!(reminders.is_empty(), "{reminders:?}");
    let event_id = match &due.target {
        nail_api::pim::ReminderTarget::Event { event_id, .. } => event_id.clone(),
        other => panic!("{other:?}"),
    };
    let Response::PimEvent { event } = ok(service, Request::GetEvent { event_id }).await else {
        panic!()
    };
    assert!(event.fields.alarms[0].acknowledged.is_some());
    service.shutdown().await;
}
