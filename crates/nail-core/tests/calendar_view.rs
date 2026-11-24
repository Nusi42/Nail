//! R7 (ADR 0246): the calendar preferences, tasks due in a range, quick
//! create from text, default calendar colours and the user's own answer
//! on event instances, through `Service::handle` like the UI.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{error, ok};
use nail_core::testing::TempService;

use nail_api::pim::{
    Calendar, CalendarComponent, CalendarPreferences, CalendarSettings, EventFields,
    ParsedEventText, PimTime, Task, TaskFields, WorkHours,
};
use nail_api::triage::Weekday;
use nail_api::{CalendarId, ErrorCode, Event, PimSourceId, Request, Response};
// The user's answer and the default colours.
use nail_api::pim::calendar::{Attendee, Organizer, ParticipationStatus};
use nail_api::pim::calendar_view::CalendarColor;
use nail_core::Service;

async fn preferences(service: &Service) -> CalendarPreferences {
    match ok(service, Request::GetCalendarPreferences).await {
        Response::CalendarPreferences { preferences } => preferences,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn preferences_default_save_and_announce() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    assert_eq!(preferences(&service).await, CalendarPreferences::default());

    let mut events = service.subscribe();
    let changed = CalendarPreferences {
        work_hours: WorkHours {
            start_minute: 9 * 60,
            end_minute: 17 * 60 + 30,
        },
        work_days: vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu],
        week_start: Some(Weekday::Mon),
        show_week_numbers: true,
        time_zone: Some("Europe/Berlin".into()),
        second_time_zone: Some("America/New_York".into()),
        show_declined: false,
    };
    let Response::CalendarPreferences { preferences: saved } = ok(
        &service,
        Request::SetCalendarPreferences {
            preferences: changed.clone(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(saved, changed);
    assert_eq!(preferences(&service).await, changed);
    let mut announced = false;
    while let Ok(event) = events.try_recv() {
        announced |= matches!(event, Event::CalendarPreferencesChanged);
    }
    assert!(announced);
}

#[tokio::test]
async fn invalid_preferences_are_refused() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let backwards = CalendarPreferences {
        work_hours: WorkHours {
            start_minute: 18 * 60,
            end_minute: 8 * 60,
        },
        ..CalendarPreferences::default()
    };
    // A well-formed name the core does not know.
    let unknown_zone = CalendarPreferences {
        time_zone: Some("Atlantis/Lost_City".into()),
        ..CalendarPreferences::default()
    };
    for preferences in [backwards, unknown_zone] {
        assert_eq!(
            error(&service, Request::SetCalendarPreferences { preferences }).await,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(preferences(&service).await, CalendarPreferences::default());
}

async fn task_list(service: &Service) -> CalendarId {
    let Response::Calendars { calendars } =
        ok(service, Request::ListCalendars { source_id: None }).await
    else {
        panic!()
    };
    calendars
        .into_iter()
        .find(|c| c.has_tasks() && c.source_id.as_str() == "local")
        .expect("a local task list")
        .id
}

async fn create_task(service: &Service, list: &CalendarId, title: &str, due: Option<PimTime>) {
    let mut fields = TaskFields::titled(title);
    fields.due = due;
    ok(
        service,
        Request::CreateTask {
            calendar_id: list.clone(),
            fields: Box::new(fields),
            recurrence: None,
        },
    )
    .await;
}

async fn due(service: &Service, from: &str, to: &str) -> Vec<Task> {
    match ok(
        service,
        Request::ListTasksDue {
            from: from.into(),
            to: to.into(),
            tz: "Europe/Berlin".into(),
            calendar_ids: None,
            include_completed: false,
        },
    )
    .await
    {
        Response::Tasks { tasks } => tasks,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn tasks_due_in_a_range_by_due() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let list = task_list(&service).await;
    create_task(&service, &list, "No due date", None).await;
    create_task(
        &service,
        &list,
        "Book the flights",
        Some(PimTime::date("2026-11-27")),
    )
    .await;
    create_task(
        &service,
        &list,
        "Plan the trip",
        Some(PimTime::zoned("2026-11-25T09:00:00", "Europe/Berlin")),
    )
    .await;
    create_task(
        &service,
        &list,
        "Next week",
        Some(PimTime::date("2026-12-03")),
    )
    .await;

    // The week of Mon 23 – Sun 29 November in Berlin (UTC+1).
    let week = due(&service, "2026-11-22T23:00:00Z", "2026-11-29T23:00:00Z").await;
    let titles: Vec<&str> = week.iter().map(|t| t.fields.title.as_str()).collect();
    assert_eq!(titles, ["Plan the trip", "Book the flights"]);
    // A due date is placed in the request's zone: Friday starts at 23:00 UTC.
    let thursday = due(&service, "2026-11-25T23:00:00Z", "2026-11-26T23:00:00Z").await;
    assert!(thursday.is_empty(), "{thursday:?}");

    assert_eq!(
        error(
            &service,
            Request::ListTasksDue {
                from: "2026-01-01T00:00:00Z".into(),
                to: "2028-01-01T00:00:00Z".into(),
                tz: "Europe/Berlin".into(),
                calendar_ids: None,
                include_completed: false,
            }
        )
        .await,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn event_text_is_checked_and_read() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let Response::EventTextParsed { parsed } = ok(
        &service,
        Request::ParseEventText {
            text: "Call the plumber".into(),
            tz: "Europe/Berlin".into(),
            now: Some("2026-11-25T09:40:00Z".into()),
            locale: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(parsed.title, "Call the plumber");
    for request in [
        Request::ParseEventText {
            text: "x".repeat(nail_api::pim::calendar_view::MAX_EVENT_TEXT + 1),
            tz: "Europe/Berlin".into(),
            now: None,
            locale: None,
        },
        Request::ParseEventText {
            text: "Lunch".into(),
            tz: "Atlantis/Lost_City".into(),
            now: None,
            locale: None,
        },
        Request::ParseEventText {
            text: "Lunch".into(),
            tz: "Europe/Berlin".into(),
            now: Some("yesterday".into()),
            locale: None,
        },
    ] {
        assert_eq!(error(&service, request).await, ErrorCode::InvalidRequest);
    }
}

/// A service whose clock (the demo clock of seeded runs) shows `now` (RFC 3339).
fn service_at(now: &str) -> TempService {
    let clock = std::sync::Arc::new(nail_core::AdjustableClock::new());
    let at = chrono::DateTime::parse_from_rfc3339(now).expect("a valid instant");
    clock.set(at.timestamp());
    TempService::quiet_with(&[], |options| {
        options.clock = Some(clock.clone());
    })
}

#[tokio::test]
async fn event_text_without_now_reads_the_core_clock() {
    // The core's clock (the demo clock in seeded runs) is a year ahead of
    // the wall clock: "tomorrow" is the day after the core's today.
    let temp = service_at("2027-11-25T10:40:00Z");
    let Response::EventTextParsed { parsed } = ok(
        &temp.service,
        Request::ParseEventText {
            text: "Lunch with Bob tomorrow 12:30".into(),
            tz: "Europe/Berlin".into(),
            now: None,
            locale: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(
        parsed.start,
        Some(PimTime::zoned("2027-11-26T12:30:00", "Europe/Berlin"))
    );
}

async fn parse(service: &Service, text: &str, now: &str) -> ParsedEventText {
    match ok(
        service,
        Request::ParseEventText {
            text: text.into(),
            tz: "Europe/Berlin".into(),
            now: Some(now.into()),
            locale: Some("en-US".into()),
        },
    )
    .await
    {
        Response::EventTextParsed { parsed } => *parsed,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn event_text_reads_relative_words_in_the_view_zone() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    // Wednesday 10:40 in Berlin: "tomorrow" is Thursday.
    let parsed = parse(
        &service,
        "Lunch with Bob tomorrow 12:30 at Corner café",
        "2026-11-25T09:40:00Z",
    )
    .await;
    assert_eq!(parsed.title, "Lunch with Bob");
    assert_eq!(parsed.location.as_deref(), Some("Corner café"));
    assert_eq!(
        parsed.start,
        Some(PimTime::zoned("2026-11-26T12:30:00", "Europe/Berlin"))
    );
    assert_eq!(
        parsed.end,
        Some(PimTime::zoned("2026-11-26T13:30:00", "Europe/Berlin"))
    );
    assert_eq!(parsed.spans.len(), 3);
    // 23:30 UTC on Wednesday is already Thursday in Berlin.
    let late = parse(&service, "Lunch tomorrow 12:30", "2026-11-25T23:30:00Z").await;
    assert_eq!(
        late.start,
        Some(PimTime::zoned("2026-11-27T12:30:00", "Europe/Berlin"))
    );
}

async fn calendars(service: &Service) -> Vec<Calendar> {
    match ok(service, Request::ListCalendars { source_id: None }).await {
        Response::Calendars { calendars } => calendars,
        other => panic!("{other:?}"),
    }
}

async fn create_calendar(service: &Service, name: &str, color: Option<&str>) -> Calendar {
    let mut settings = CalendarSettings::named(name);
    settings.color = color.map(str::to_owned);
    match ok(
        service,
        Request::CreateCalendar {
            source_id: PimSourceId::local(),
            settings,
            components: vec![CalendarComponent::Event],
        },
    )
    .await
    {
        Response::CalendarSaved { calendar, .. } => *calendar,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn new_calendars_get_a_colour_no_other_calendar_has() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    // The local calendar and task list already have the first colours.
    let mut used: Vec<String> = calendars(&service)
        .await
        .into_iter()
        .map(|c| c.settings.color.expect("a default colour"))
        .collect();
    assert_eq!(
        used,
        [CalendarColor::Blue.value(), CalendarColor::Cyan.value()]
    );
    for name in ["Work", "Family", "Sport"] {
        let created = create_calendar(&service, name, None).await;
        let color = created.settings.color.expect("a default colour");
        assert!(CalendarColor::of_value(&color).is_some(), "{color}");
        assert!(!used.contains(&color), "{color} is taken: {used:?}");
        used.push(color);
    }
    // A chosen colour is kept, even one another calendar has.
    let chosen = create_calendar(&service, "Mine", Some("#2563eb")).await;
    assert_eq!(chosen.settings.color.as_deref(), Some("#2563eb"));
}

const ME: &str = "me@example.com";
const ALIAS: &str = "alias@example.com";

#[tokio::test]
async fn instances_carry_the_users_own_answer() {
    let accounts = format!(
        "account \"home\" email=\"{ME}\" name=\"Home\" {{
             imap host=\"127.0.0.1\" port=1 tls=\"plaintext\"
             smtp host=\"127.0.0.1\" port=1 tls=\"plaintext\"
             identity \"{ME}\" name=\"Me\"
             identity \"{ALIAS}\" name=\"Me too\"
}}
"
    );
    let (service, _d, _c) =
        TempService::quiet_with(&[("accounts.kdl", &accounts)], |_| {}).into_parts();
    let calendar = calendars(&service)
        .await
        .into_iter()
        .find(Calendar::has_events)
        .expect("the local calendar")
        .id;
    let meeting = |title: &str, organizer: &str, me: Option<(&str, ParticipationStatus)>| {
        let mut fields = EventFields::new(
            title,
            PimTime::utc("2026-11-26T09:00:00"),
            PimTime::utc("2026-11-26T10:00:00"),
        );
        fields.organizer = Some(Organizer {
            email: organizer.into(),
            name: None,
        });
        fields.attendees.push(Attendee::new("ann@example.org"));
        if let Some((email, status)) = me {
            let mut attendee = Attendee::new(email);
            attendee.status = status;
            fields.attendees.push(attendee);
        }
        fields
    };
    let events = [
        meeting(
            "Invited",
            "ann@example.org",
            Some(("ME@example.com", ParticipationStatus::NeedsAction)),
        ),
        meeting(
            "Via the alias",
            "ann@example.org",
            Some((ALIAS, ParticipationStatus::Tentative)),
        ),
        meeting(
            "Declined",
            "ann@example.org",
            Some((ME, ParticipationStatus::Declined)),
        ),
        meeting("Organized", ME, Some((ME, ParticipationStatus::Accepted))),
        meeting("Not mine", "ann@example.org", None),
    ];
    for fields in events {
        ok(
            &service,
            Request::CreateEvent {
                calendar_id: calendar.clone(),
                fields: Box::new(fields),
                recurrence: None,
            },
        )
        .await;
    }
    let Response::EventInstances { instances, .. } = ok(
        &service,
        Request::ListEventInstances {
            from: "2026-11-26T00:00:00Z".into(),
            to: "2026-11-27T00:00:00Z".into(),
            calendar_ids: None,
            tz: "UTC".into(),
        },
    )
    .await
    else {
        panic!()
    };
    let status = |title: &str| {
        instances
            .iter()
            .find(|i| i.title == title)
            .unwrap_or_else(|| panic!("{title}"))
            .my_status
    };
    assert_eq!(status("Invited"), Some(ParticipationStatus::NeedsAction));
    assert_eq!(
        status("Via the alias"),
        Some(ParticipationStatus::Tentative)
    );
    assert_eq!(status("Declined"), Some(ParticipationStatus::Declined));
    assert_eq!(status("Organized"), None);
    assert_eq!(status("Not mine"), None);

    // What "Organized" overlaps: the invitation card's rule, so the declined one does not count.
    let organized = instances
        .iter()
        .find(|i| i.title == "Organized")
        .expect("Organized")
        .event_id
        .clone();
    let Response::EventOverlaps { overlaps } = ok(
        &service,
        Request::ListEventOverlaps {
            event_id: organized,
            from: "2026-11-26T09:00:00Z".into(),
            to: "2026-11-26T10:00:00Z".into(),
            tz: "UTC".into(),
        },
    )
    .await
    else {
        panic!()
    };
    let mut titles: Vec<&str> = overlaps.iter().map(|o| o.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Invited", "Not mine", "Via the alias"]);
}

#[tokio::test]
async fn search_files_a_repeating_event_by_its_next_occurrence() {
    use nail_api::pim::calendar::Frequency;
    use nail_api::pim::{PimItemKind, Recurrence, RecurrenceEnd, RecurrenceRule};
    let temp = service_at("2026-11-25T10:40:00Z");
    let service = &temp.service;
    let calendar = calendars(service)
        .await
        .into_iter()
        .find(|c| c.components.contains(&CalendarComponent::Event))
        .expect("a local calendar")
        .id;
    let berlin = |local: &str| PimTime::zoned(local, "Europe/Berlin");
    let create = |title: &str, start: &str, end: &str, recurrence: Option<Recurrence>| {
        Request::CreateEvent {
            calendar_id: calendar.clone(),
            fields: Box::new(EventFields::new(title, berlin(start), berlin(end))),
            recurrence,
        }
    };
    // A daily stand-up from Tuesday 24 at 8:30: today's has passed, the next is tomorrow.
    let daily = Recurrence::rule(RecurrenceRule::new(Frequency::Daily));
    ok(
        service,
        create(
            "Standup daily",
            "2026-11-24T08:30:00",
            "2026-11-24T08:45:00",
            Some(daily),
        ),
    )
    .await;
    // A series that ended: its last occurrence.
    let mut three = RecurrenceRule::new(Frequency::Weekly);
    three.end = RecurrenceEnd::Count { count: 3 };
    ok(
        service,
        create(
            "Standup weekly",
            "2026-10-01T09:00:00",
            "2026-10-01T10:00:00",
            Some(Recurrence::rule(three)),
        ),
    )
    .await;
    // A single event keeps its start.
    ok(
        service,
        create(
            "Standup once",
            "2026-11-20T09:00:00",
            "2026-11-20T10:00:00",
            None,
        ),
    )
    .await;
    let Response::PimSearchResults { hits } = ok(
        service,
        Request::SearchPim {
            query: "standup".into(),
            kinds: vec![PimItemKind::Event],
            limit: 10,
        },
    )
    .await
    else {
        panic!()
    };
    let start = |title: &str| {
        hits.iter()
            .find(|h| h.title == title)
            .and_then(|h| h.start.clone())
            .unwrap_or_else(|| panic!("no hit {title}: {hits:?}"))
    };
    assert_eq!(start("Standup daily"), berlin("2026-11-26T08:30:00"));
    assert_eq!(start("Standup weekly"), berlin("2026-10-15T09:00:00"));
    assert_eq!(start("Standup once"), berlin("2026-11-20T09:00:00"));
}
