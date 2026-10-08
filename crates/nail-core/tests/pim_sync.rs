//! M3a integration (ADR 0063): the PIM source engines against in-process
//! mock servers — CalDAV / CardDAV both ways (`MockDavServer`), a
//! read-only ICS subscription (`MockHttpServer`) — and reminders firing
//! at the controlled clock time.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{ok, until_some};
use nail_core::testing::{TempService, error_of, plain_options};

use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::pim::{
    Alarm, ContactFields, EventFields, EventInstance, LabeledValue, PimSourceState, PimTime,
};
use nail_api::triage::NotificationKind;
use nail_api::{CalendarId, ErrorCode, Event, Request, Response, SecretRef};
use nail_core::{AdjustableClock, EngineTiming, Service};
use nail_testkit_net::{MockDavConfig, MockDavServer, MockHttpServer, MockResponse};

struct Fixture {
    service: Service,
    _temp: TempService,
    clock: Arc<AdjustableClock>,
}

fn start(accounts_kdl: &str, secrets: &[(&str, &str)]) -> Fixture {
    let store = Arc::new(MemorySecretStore::new());
    for (reference, value) in secrets {
        store
            .set(&SecretRef((*reference).to_owned()), value)
            .unwrap();
    }
    let clock = Arc::new(AdjustableClock::new());
    let temp = TempService::quiet_with(&[("accounts.kdl", accounts_kdl)], |options| {
        options.start_engines = true;
        options.secrets = Some(store);
        let mut timing = EngineTiming::fast();
        timing.poll_override = Some(Duration::from_secs(30));
        options.timing = timing;
        options.clock = Some(clock.clone());
    });
    Fixture {
        service: temp.service.clone(),
        _temp: temp,
        clock,
    }
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

/// Creating an event in `calendar` is refused (a read-only calendar).
async fn refuses_events(service: &Service, calendar: &CalendarId) {
    let refused = service
        .handle(Request::CreateEvent {
            calendar_id: calendar.clone(),
            fields: Box::new(EventFields::new(
                "Mine",
                PimTime::date("2026-10-09"),
                PimTime::date("2026-10-10"),
            )),
            recurrence: None,
        })
        .await;
    assert_eq!(error_of(&refused).0, ErrorCode::Forbidden, "{refused:?}");
}

/// A mock HTTP server serving the holidays feed with `headers`.
async fn holidays_server(headers: Vec<(String, String)>) -> MockHttpServer {
    let http = MockHttpServer::start().await.unwrap();
    http.get(
        None,
        "/holidays.ics",
        MockResponse {
            status: 200,
            content_type: "text/calendar".into(),
            body: FEED.as_bytes().to_vec(),
            headers,
            delay: None,
        },
    );
    http
}

/// The calendar of the subscription "town", once it is there.
async fn town_calendar(service: &Service) -> nail_api::pim::Calendar {
    until_some("the feed's calendar", || async {
        calendars(service, "town").await.into_iter().next()
    })
    .await
}

/// A service without sources whose clock stands an hour before
/// 2026-10-05 09:00 UTC (returned), and its local calendar.
async fn reminder_fixture() -> (Fixture, i64, CalendarId) {
    let f = start("", &[]);
    let start_at = chrono::DateTime::parse_from_rfc3339("2026-10-05T09:00:00Z")
        .unwrap()
        .timestamp();
    f.clock.set(start_at - 3600);
    let Response::Calendars { calendars } =
        ok(&f.service, Request::ListCalendars { source_id: None }).await
    else {
        panic!("calendars expected")
    };
    let calendar = calendars
        .iter()
        .find(|c| c.has_events())
        .unwrap()
        .id
        .clone();
    (f, start_at, calendar)
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
    );
    let service = &f.service;
    // Discovery found the seeded calendars (the holidays one read-only).
    let found = until_some("the DAV calendars", || async {
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
    until_some("the upload", || async {
        let collection = dav.collection(&path)?;
        collection
            .objects
            .values()
            .any(|o| o.data.contains("SUMMARY:Uploaded"))
            .then_some(())
    })
    .await;
    until_some("the synced state", || async {
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
    until_some("the server's event", || async {
        instances(service, &personal.id)
            .await
            .iter()
            .any(|i| i.title == "From another client")
            .then_some(())
    })
    .await;
    // The read-only calendar refuses writes.
    let holidays = found.iter().find(|c| c.read_only).unwrap();
    refuses_events(service, &holidays.id).await;
    // CardDAV: a contact is uploaded as vCard.
    let book = until_some("the address book", || async {
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
    until_some("the vCard upload", || async {
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
    until_some("the server delete", || async {
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
    let f = start(&kdl, &[("keychain:nail/pim/dav/password", "wrong")]);
    let service = &f.service;
    until_some("the auth error", || async {
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
    let http = holidays_server(vec![("ETag".into(), "\"v1\"".into())]).await;
    let kdl = format!(
        "calendar-subscription \"town\" url=\"{}/holidays.ics\" color=\"#cc3333\"\n",
        http.base_url()
    );
    let f = start(&kdl, &[]);
    let service = &f.service;
    let calendar = town_calendar(service).await;
    assert!(calendar.read_only);
    assert_eq!(calendar.settings.color.as_deref(), Some("#cc3333"));
    let list = until_some("the feed's events", || async {
        let list = instances(service, &calendar.id).await;
        (!list.is_empty()).then_some(list)
    })
    .await;
    assert_eq!(list[0].title, "Unity Day");
    assert!(list[0].all_day && list[0].read_only);
    refuses_events(service, &calendar.id).await;
    // The feed's ETag is kept for the next, conditional fetch.
    let sync = service
        .store()
        .collection_sync(calendar.id.as_str())
        .unwrap();
    assert_eq!(sync.etag.as_deref(), Some("\"v1\""));
    ok(service, Request::SyncPimSources { source_id: None }).await;
    until_some("a second fetch", || async {
        (http.requests().len() >= 2).then_some(())
    })
    .await;
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reminders_fire_at_the_clock_time() {
    let (f, start_at, calendar) = reminder_fixture().await;
    let service = &f.service;
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
        other @ nail_api::pim::ReminderTarget::Task { .. } => panic!("{other:?}"),
    };
    let Response::PimEvent { event } = ok(service, Request::GetEvent { event_id }).await else {
        panic!()
    };
    assert!(event.fields.alarms[0].acknowledged.is_some());
    service.shutdown().await;
}

/// Hostile or careless calendars cannot flood reminders (ADR 0064): a
/// series repeating every second plans none, an alarm repeating every
/// minute keeps a bounded plan, and many reminders due at once become one
/// summary notification.
#[tokio::test(flavor = "multi_thread")]
async fn reminder_floods_are_bounded() {
    let (f, start_at, calendar) = reminder_fixture().await;
    let service = &f.service;
    let create = |title: &str, rule: Option<&str>, alarm: Alarm| {
        let mut fields = EventFields::new(
            title,
            PimTime::utc("2026-10-05T09:00:00"),
            PimTime::utc("2026-10-05T09:30:00"),
        );
        fields.alarms.push(alarm);
        let recurrence = rule.map(|r| {
            nail_api::pim::Recurrence::rule(
                nail_ical::recur::parse_rrule(r, &PimTime::utc("2026-10-05T09:00:00")).unwrap(),
            )
        });
        Request::CreateEvent {
            calendar_id: calendar.clone(),
            fields: Box::new(fields),
            recurrence,
        }
    };
    ok(
        service,
        create(
            "Every second",
            Some("FREQ=SECONDLY"),
            Alarm::minutes_before(15),
        ),
    )
    .await;
    let mut repeating = Alarm::minutes_before(15);
    repeating.repeat = Some(nail_api::pim::calendar::AlarmRepeat {
        count: 100,
        interval_secs: 60,
    });
    ok(service, create("Nagging", None, repeating)).await;
    // Only the next MAX_AHEAD_PER_ITEM of the nagging alarm are planned,
    // none of the series repeating every second.
    let planned = until_some("the planned reminders", || async {
        let records = service.store().reminder_records(&[]).unwrap();
        (!records.is_empty()).then_some(records)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let planned_again = service.store().reminder_records(&[]).unwrap();
    assert_eq!(planned.len(), planned_again.len());
    assert_eq!(planned.len(), nail_triage::reminders::MAX_AHEAD_PER_ITEM);
    let items: std::collections::HashSet<&str> =
        planned.iter().map(|r| r.item_id.as_str()).collect();
    assert_eq!(items.len(), 1);
    for i in 0..5 {
        ok(
            service,
            create(&format!("Meeting {i}"), None, Alarm::minutes_before(15)),
        )
        .await;
    }
    let mut events = service.subscribe();
    // At 08:45 six reminders are due at once: one summary notification.
    f.clock.set(start_at - 15 * 60);
    let mut due = 0;
    let mut notifications = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while (due < 6 || notifications.is_empty()) && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            Ok(Ok(Event::ReminderDue { .. })) => due += 1,
            Ok(Ok(Event::NotificationRequested { notification }))
                if notification.kind == NotificationKind::Reminder =>
            {
                notifications.push(notification);
            }
            _ => {}
        }
    }
    assert_eq!(due, 6);
    // Late arrivals would show up here.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while let Ok(event) = events.try_recv() {
        if let Event::NotificationRequested { notification } = event {
            notifications.push(notification);
        }
    }
    assert_eq!(notifications.len(), 1, "{notifications:?}");
    assert_eq!(notifications[0].tag, "reminders");
    assert_eq!(notifications[0].title.id, "notification-reminders-title");
    service.shutdown().await;
}

const FEED_WITH_ALARM: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Town//EN\r\nX-WR-CALNAME:Town\r\n\
BEGIN:VEVENT\r\nUID:m1@town\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261007T180000Z\r\n\
DTEND:20261007T190000Z\r\nSUMMARY:Council meeting\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\n\
DESCRIPTION:Reminder\r\nTRIGGER:-PT30M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// Changing a subscription in accounts.kdl takes effect on the next
/// fetch: `alarms` switches the calendar's reminders and refetches the
/// feed without the stored ETag (a `304` would keep the items without
/// their alarms), a new name renames the calendar; unchanged
/// subscriptions keep fetching conditionally.
#[tokio::test(flavor = "multi_thread")]
async fn changed_subscriptions_take_effect() {
    use nail_proxy::testing::{Reply, Upstream};
    let upstream = Upstream::start().await.unwrap();
    upstream.route(
        "/town.ics",
        Reply::ok("text/calendar", FEED_WITH_ALARM).header("ETag", "\"v1\""),
    );
    let url = format!("http://{}/town.ics", upstream.addr());
    let kdl = format!("calendar-subscription \"town\" url=\"{url}\"\n");
    let f = start(&kdl, &[]);
    let service = &f.service;
    let calendar = town_calendar(service).await;
    let list = until_some("the feed's events", || async {
        let list = instances(service, &calendar.id).await;
        (!list.is_empty()).then_some(list)
    })
    .await;
    assert!(!calendar.settings.reminders);
    assert!(!list[0].has_alarms, "alarms are off for subscriptions");
    let first_requests = upstream.requests().len();
    assert_eq!(first_requests, 1);

    let config = nail_api::pim::PimSourceConfig {
        id: "town".into(),
        name: Some("Council".into()),
        enabled: true,
        settings: nail_api::pim::source::PimSourceSettings::Ics {
            url: url.clone(),
            color: None,
            alarms: true,
            refresh_minutes: None,
            auth: None,
        },
    };
    ok(
        service,
        Request::UpdatePimSource {
            config: Box::new(config),
            secret: None,
        },
    )
    .await;
    let calendar = until_some("the changed calendar", || async {
        calendars(service, "town")
            .await
            .into_iter()
            .next()
            .filter(|c| c.settings.reminders && c.settings.name == "Council")
    })
    .await;
    let list = until_some("the feed's alarms", || async {
        let list = instances(service, &calendar.id).await;
        list.first().is_some_and(|i| i.has_alarms).then_some(list)
    })
    .await;
    assert_eq!(list[0].title, "Council meeting");
    let requests = upstream.requests();
    assert!(requests.len() > first_requests);
    assert_eq!(
        requests[first_requests].header("if-none-match"),
        None,
        "the changed subscription is fetched in full"
    );
    // Unchanged: conditional again.
    ok(service, Request::SyncPimSources { source_id: None }).await;
    let requests = until_some("a conditional fetch", || async {
        let requests = upstream.requests();
        (requests.len() > first_requests + 1).then_some(requests)
    })
    .await;
    assert_eq!(
        requests.last().unwrap().header("if-none-match"),
        Some("\"v1\"")
    );
    service.shutdown().await;
}

/// A subscription to a service on this computer is refused outside tests
/// and seeded demos (a `webcal://localhost` link must not make Nail poll
/// local services).
#[tokio::test(flavor = "multi_thread")]
async fn loopback_subscriptions_are_refused_in_real_runs() {
    let http = holidays_server(Vec::new()).await;
    let kdl = format!(
        "calendar-subscription \"town\" url=\"webcal://localhost:{}/holidays.ics\"\n",
        http.addr().port()
    );
    let temp = TempService::open_with(&[("accounts.kdl", &kdl)], |data, config| {
        let mut options = nail_core::demo::demo_options(plain_options(data, config));
        options.timing = EngineTiming::fast();
        // As in a real (not seeded) run.
        options.loopback_feeds = false;
        options
    });
    let service = temp.service.clone();
    let source = until_some("the refused subscription", || async {
        let Response::PimSources { sources } = ok(&service, Request::ListPimSources).await else {
            panic!()
        };
        sources
            .into_iter()
            .find(|s| s.id.as_str() == "town" && s.error.is_some())
    })
    .await;
    assert_eq!(source.error, Some(ErrorCode::Forbidden));
    assert_eq!(http.requests().len(), 0);
    service.shutdown().await;
}

/// WP75's conflict rule end to end: a local edit of an event another
/// client changed meanwhile meets `412` on replay; the server's version
/// wins and is shown, the local one is kept as a conflict copy; resolving
/// with `keep: local` writes it back to the server, `keep: server` drops
/// it.
#[tokio::test(flavor = "multi_thread")]
async fn conflicts_keep_the_server_version_and_resolve_both_ways() {
    let dav = MockDavServer::start(MockDavConfig::default())
        .await
        .unwrap();
    let path = dav.calendar_path("personal");
    dav.put_object(&path, "server-1.ics", SERVER_EVENT).unwrap();
    let kdl = format!(
        "caldav \"dav\" url=\"{}/\" {{\n    auth \"password\" username=\"ada\"\n}}\n",
        dav.base_url()
    );
    let f = start(&kdl, &[("keychain:nail/pim/dav/password", "secret")]);
    let service = &f.service;
    let personal = until_some("the personal calendar", || async {
        calendars(service, "dav")
            .await
            .into_iter()
            .find(|c| c.settings.name == "Personal")
    })
    .await;
    let event_id = until_some("the server's event", || async {
        instances(service, &personal.id)
            .await
            .into_iter()
            .find(|i| i.title == "From another client")
            .map(|i| i.event_id)
    })
    .await;
    let get = |id: nail_api::EventId| async move {
        let Response::PimEvent { event } = ok(service, Request::GetEvent { event_id: id }).await
        else {
            panic!()
        };
        event
    };
    let edit = |title: &'static str, event: Box<nail_api::pim::CalendarEvent>| async move {
        let mut fields = event.fields.clone();
        fields.title = title.into();
        ok(
            service,
            Request::UpdateEvent {
                event_id: event.id.clone(),
                scope: nail_api::pim::EditScope::All,
                recurrence_id: None,
                fields: Box::new(fields),
                recurrence: None,
            },
        )
        .await;
    };

    // Another client changes it (no sync yet), then we edit our copy.
    dav.put_object(
        &path,
        "server-1.ics",
        &SERVER_EVENT.replace("From another client", "Changed elsewhere"),
    )
    .unwrap();
    edit("Our edit", get(event_id.clone()).await).await;
    let conflicted = until_some("the conflict", || async {
        let event = get(event_id.clone()).await;
        (event.sync == nail_api::pim::PimItemSync::Conflict).then_some(event)
    })
    .await;
    assert_eq!(conflicted.fields.title, "Changed elsewhere");
    let object = service
        .store()
        .pim_object(&nail_api::pim::PimItemRef::Event {
            event_id: event_id.clone(),
        })
        .unwrap()
        .unwrap();
    assert!(object.conflict_raw.unwrap().contains("SUMMARY:Our edit"));
    let server_text = || dav.object(&path, "server-1.ics").unwrap().data;
    assert!(server_text().contains("Changed elsewhere"));

    // Keep ours: written back over the server's version.
    ok(
        service,
        Request::ResolvePimConflict {
            item: nail_api::pim::PimItemRef::Event {
                event_id: event_id.clone(),
            },
            keep: nail_api::pim::ConflictChoice::Local,
        },
    )
    .await;
    until_some("our version on the server", || async {
        server_text().contains("SUMMARY:Our edit").then_some(())
    })
    .await;
    let resolved = until_some("the synced event", || async {
        let event = get(event_id.clone()).await;
        (event.sync == nail_api::pim::PimItemSync::Synced).then_some(event)
    })
    .await;
    assert_eq!(resolved.fields.title, "Our edit");

    // Again, but keep the server's: the copy goes, nothing is written.
    dav.put_object(
        &path,
        "server-1.ics",
        &SERVER_EVENT.replace("From another client", "Second change"),
    )
    .unwrap();
    edit("Second edit", resolved).await;
    until_some("the second conflict", || async {
        let event = get(event_id.clone()).await;
        (event.sync == nail_api::pim::PimItemSync::Conflict).then_some(())
    })
    .await;
    ok(
        service,
        Request::ResolvePimConflict {
            item: nail_api::pim::PimItemRef::Event {
                event_id: event_id.clone(),
            },
            keep: nail_api::pim::ConflictChoice::Server,
        },
    )
    .await;
    let kept = get(event_id.clone()).await;
    assert_eq!(kept.sync, nail_api::pim::PimItemSync::Synced);
    assert_eq!(kept.fields.title, "Second change");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(server_text().contains("Second change"));
    let object = service
        .store()
        .pim_object(&nail_api::pim::PimItemRef::Event { event_id })
        .unwrap()
        .unwrap();
    assert!(object.conflict_raw.is_none());
    service.shutdown().await;
}
