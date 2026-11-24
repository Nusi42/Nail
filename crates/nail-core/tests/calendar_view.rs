//! R7 (ADR 0246): the calendar preferences, tasks due in a range and quick
//! create from text, through `Service::handle` like the UI.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{error, ok};
use nail_core::testing::TempService;

use nail_api::pim::{CalendarPreferences, PimTime, Task, TaskFields, WorkHours};
use nail_api::triage::Weekday;
use nail_api::{CalendarId, ErrorCode, Event, Request, Response};
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
