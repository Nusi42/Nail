//! M3a integration (ADR 0063): calendars, events with edit scopes,
//! instances, free time, tasks with subtasks, contacts, groups,
//! duplicates and merging, imports, search and address autocomplete —
//! the local source, through `Service::handle` like the UI.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::ok;
use nail_core::testing::TempService;

use nail_api::pim::calendar::Frequency;
use nail_api::pim::import::PimImportSource;
use nail_api::pim::{
    CalendarEvent, ContactFields, EditScope, EventFields, EventInstance, LabeledValue, PimItemKind,
    PimTime, Recurrence, RecurrenceRule, Task, TaskFields, TaskStatus,
};
use nail_api::{CalendarId, ErrorCode, Request, Response};
use nail_core::Service;

async fn error(service: &Service, request: Request) -> ErrorCode {
    match service.handle(request).await {
        Response::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn berlin(local: &str) -> PimTime {
    PimTime::zoned(local, "Europe/Berlin")
}

async fn calendars(service: &Service) -> Vec<nail_api::pim::Calendar> {
    match ok(service, Request::ListCalendars { source_id: None }).await {
        Response::Calendars { calendars } => calendars,
        other => panic!("{other:?}"),
    }
}

async fn default_calendar(service: &Service) -> CalendarId {
    calendars(service)
        .await
        .into_iter()
        .find(|c| c.has_events() && c.source_id.as_str() == "local")
        .expect("a local calendar")
        .id
}

async fn default_list(service: &Service) -> CalendarId {
    calendars(service)
        .await
        .into_iter()
        .find(|c| c.has_tasks() && c.source_id.as_str() == "local")
        .expect("a local task list")
        .id
}

async fn instances(service: &Service, from: &str, to: &str) -> Vec<EventInstance> {
    match ok(
        service,
        Request::ListEventInstances {
            from: from.into(),
            to: to.into(),
            calendar_ids: None,
            tz: "Europe/Berlin".into(),
        },
    )
    .await
    {
        Response::EventInstances { instances, .. } => instances,
        other => panic!("{other:?}"),
    }
}

fn saved_event(response: Response) -> (CalendarEvent, Option<CalendarEvent>) {
    match response {
        Response::EventSaved { event, split, .. } => (*event, split.map(|s| *s)),
        other => panic!("{other:?}"),
    }
}

/// Imports `source` where its items belong (default calendar / book).
async fn import(
    service: &Service,
    source: PimImportSource,
) -> nail_api::pim::import::PimImportResult {
    let Response::PimImported { result } = ok(
        service,
        Request::ImportPim {
            source,
            calendar_id: None,
            address_book_id: None,
        },
    )
    .await
    else {
        panic!("import result expected")
    };
    *result
}

#[tokio::test]
async fn local_defaults_exist() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let list = calendars(&service).await;
    assert!(list.iter().any(nail_api::Calendar::has_events), "{list:?}");
    assert!(list.iter().any(nail_api::Calendar::has_tasks));
    let Response::AddressBooks { address_books } =
        ok(&service, Request::ListAddressBooks { source_id: None }).await
    else {
        panic!()
    };
    // The default book and the Collected one.
    assert_eq!(address_books.len(), 2, "{address_books:?}");
    let Response::PimSources { sources } = ok(&service, Request::ListPimSources).await else {
        panic!()
    };
    assert_eq!(sources[0].id.as_str(), "local");
}

#[tokio::test]
async fn recurring_events_with_every_scope() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let calendar = default_calendar(&service).await;
    let mut weekly = RecurrenceRule::new(Frequency::Weekly);
    weekly.end = nail_api::pim::RecurrenceEnd::Count { count: 4 };
    let (event, _) = saved_event(
        ok(
            &service,
            Request::CreateEvent {
                calendar_id: calendar.clone(),
                fields: Box::new(EventFields::new(
                    "Planning",
                    berlin("2026-10-05T09:00:00"),
                    berlin("2026-10-05T10:00:00"),
                )),
                recurrence: Some(Recurrence::rule(weekly)),
            },
        )
        .await,
    );
    assert!(event.is_recurring());
    let all = instances(&service, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z").await;
    assert_eq!(all.len(), 4, "{all:?}");
    // Summer time ends on 25 October: the wall clock stays 09:00.
    assert_eq!(all[0].start_utc, "2026-10-05T07:00:00Z");
    assert_eq!(all[3].start_utc, "2026-10-26T08:00:00Z");

    // "This": the second occurrence moves to 11:00.
    let second = all[1].recurrence_id.clone().unwrap();
    let mut moved = EventFields::new(
        "Planning",
        berlin("2026-10-12T11:00:00"),
        berlin("2026-10-12T12:00:00"),
    );
    moved.location = Some("Room 2".into());
    ok(
        &service,
        Request::UpdateEvent {
            event_id: event.id.clone(),
            scope: EditScope::This,
            recurrence_id: Some(second),
            fields: Box::new(moved),
            recurrence: None,
        },
    )
    .await;
    let all = instances(&service, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z").await;
    assert!(all[1].overridden && all[1].location.as_deref() == Some("Room 2"));

    // "This and following" from the third: a new series with the change.
    let third = all[2].recurrence_id.clone().unwrap();
    let (old, split) = saved_event(
        ok(
            &service,
            Request::UpdateEvent {
                event_id: event.id.clone(),
                scope: EditScope::ThisAndFollowing,
                recurrence_id: Some(third),
                fields: Box::new(EventFields::new(
                    "Planning (new)",
                    berlin("2026-10-19T09:30:00"),
                    berlin("2026-10-19T10:30:00"),
                )),
                recurrence: None,
            },
        )
        .await,
    );
    let split = split.expect("a split series");
    assert_ne!(split.id, old.id);
    let all = instances(&service, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z").await;
    let titles: Vec<&str> = all.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Planning", "Planning", "Planning (new)", "Planning (new)"],
        "{all:?}"
    );
    assert_eq!(all[2].start_utc, "2026-10-19T07:30:00Z");

    // "This" on delete: the last occurrence goes.
    let last = all[3].recurrence_id.clone().unwrap();
    ok(
        &service,
        Request::DeleteEvent {
            event_id: split.id.clone(),
            scope: EditScope::This,
            recurrence_id: Some(last),
        },
    )
    .await;
    assert_eq!(
        instances(&service, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z")
            .await
            .len(),
        3
    );
    // A scope without an occurrence is refused with the issue's text.
    match service
        .handle(Request::UpdateEvent {
            event_id: event.id.clone(),
            scope: EditScope::This,
            recurrence_id: None,
            fields: Box::new(EventFields::new(
                "x",
                berlin("2026-10-05T09:00:00"),
                berlin("2026-10-05T10:00:00"),
            )),
            recurrence: None,
        })
        .await
    {
        Response::Error {
            code, localized, ..
        } => {
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert_eq!(localized.unwrap().id, "pim-issue-invalid-scope");
        }
        other => panic!("{other:?}"),
    }
    // "All": the whole first series goes.
    ok(
        &service,
        Request::DeleteEvent {
            event_id: event.id.clone(),
            scope: EditScope::All,
            recurrence_id: None,
        },
    )
    .await;
    assert_eq!(
        instances(&service, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z")
            .await
            .len(),
        1
    );
    // Search finds the series by title; the iCalendar text is stored.
    let Response::PimSearchResults { hits } = ok(
        &service,
        Request::SearchPim {
            query: "planning".into(),
            kinds: vec![PimItemKind::Event],
            limit: 10,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(hits.len(), 1, "{hits:?}");
}

#[tokio::test]
async fn invalid_events_are_refused() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let calendar = default_calendar(&service).await;
    let mut fields = EventFields::new(
        "Backwards",
        berlin("2026-10-05T10:00:00"),
        PimTime::utc("2026-10-05T07:00:00"),
    );
    let create = |fields: EventFields| Request::CreateEvent {
        calendar_id: calendar.clone(),
        fields: Box::new(fields),
        recurrence: None,
    };
    assert_eq!(
        error(&service, create(fields.clone())).await,
        ErrorCode::InvalidRequest
    );
    fields.end = PimTime::zoned("2026-10-05T11:00:00", "Mars/Base");
    assert_eq!(
        error(&service, create(fields)).await,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        error(
            &service,
            Request::CreateEvent {
                calendar_id: "nope".into(),
                fields: Box::new(EventFields::new(
                    "x",
                    PimTime::date("2026-10-05"),
                    PimTime::date("2026-10-06")
                )),
                recurrence: None,
            }
        )
        .await,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn free_time_skips_busy_events() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let calendar = default_calendar(&service).await;
    ok(
        &service,
        Request::CreateEvent {
            calendar_id: calendar,
            fields: Box::new(EventFields::new(
                "Busy",
                PimTime::utc("2026-10-05T10:00:00"),
                PimTime::utc("2026-10-05T11:00:00"),
            )),
            recurrence: None,
        },
    )
    .await;
    let Response::FreeTime { slots } = ok(
        &service,
        Request::FindFreeTime {
            from: "2026-10-05T09:00:00Z".into(),
            to: "2026-10-05T12:00:00Z".into(),
            duration_minutes: 30,
            calendar_ids: None,
            tz: "UTC".into(),
        },
    )
    .await
    else {
        panic!()
    };
    let spans: Vec<(&str, &str)> = slots
        .iter()
        .map(|s| (s.start.as_str(), s.end.as_str()))
        .collect();
    assert_eq!(
        spans,
        [
            ("2026-10-05T09:00:00Z", "2026-10-05T10:00:00Z"),
            ("2026-10-05T11:00:00Z", "2026-10-05T12:00:00Z")
        ]
    );
}

fn saved_task(response: Response) -> Task {
    match response {
        Response::TaskSaved { task, .. } => *task,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn tasks_with_subtasks_and_recurrence() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let list = default_list(&service).await;
    let parent = saved_task(
        ok(
            &service,
            Request::CreateTask {
                calendar_id: list.clone(),
                fields: Box::new(TaskFields::titled("Move house")),
                recurrence: None,
            },
        )
        .await,
    );
    let mut sub = TaskFields::titled("Pack books");
    sub.parent_id = Some(parent.id.clone());
    let child = saved_task(
        ok(
            &service,
            Request::CreateTask {
                calendar_id: list.clone(),
                fields: Box::new(sub),
                recurrence: None,
            },
        )
        .await,
    );
    let Response::PimTask { task } = ok(
        &service,
        Request::GetTask {
            task_id: parent.id.clone(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(task.subtask_ids, std::slice::from_ref(&child.id));
    // A task cannot become its own grandparent.
    let mut cycle = parent.fields.clone();
    cycle.parent_id = Some(child.id.clone());
    match service
        .handle(Request::UpdateTask {
            task_id: parent.id.clone(),
            fields: Box::new(cycle),
            recurrence: None,
        })
        .await
    {
        Response::Error { localized, .. } => {
            assert_eq!(localized.unwrap().id, "pim-issue-task-cycle");
        }
        other => panic!("{other:?}"),
    }
    let done = saved_task(
        ok(
            &service,
            Request::CompleteTask {
                task_id: child.id.clone(),
                completed: true,
            },
        )
        .await,
    );
    assert_eq!(done.fields.status, TaskStatus::Completed);
    assert!(done.fields.completed_at.is_some());
    // A recurring task moves on when completed.
    let mut weekly = TaskFields::titled("Water plants");
    weekly.due = Some(PimTime::date("2026-10-05"));
    let recurring = saved_task(
        ok(
            &service,
            Request::CreateTask {
                calendar_id: list.clone(),
                fields: Box::new(weekly),
                recurrence: Some(Recurrence::rule(RecurrenceRule::new(Frequency::Weekly))),
            },
        )
        .await,
    );
    let next = saved_task(
        ok(
            &service,
            Request::CompleteTask {
                task_id: recurring.id.clone(),
                completed: true,
            },
        )
        .await,
    );
    assert_eq!(next.fields.due, Some(PimTime::date("2026-10-12")));
    assert_eq!(next.fields.status, TaskStatus::NeedsAction);
    // Open tasks only (the completed subtask is left out).
    let Response::Tasks { tasks } = ok(
        &service,
        Request::ListTasks {
            calendar_ids: None,
            include_completed: false,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(tasks.len(), 2, "{tasks:?}");
    // Deleting the parent with its subtasks.
    ok(
        &service,
        Request::DeleteTask {
            task_id: parent.id.clone(),
            with_subtasks: true,
        },
    )
    .await;
    assert_eq!(
        error(&service, Request::GetTask { task_id: child.id }).await,
        ErrorCode::NotFound
    );
}

fn person(name: &str, email: &str) -> ContactFields {
    let mut fields = ContactFields::named(name);
    fields.emails.push(LabeledValue::new(email));
    fields
}

#[tokio::test]
async fn contacts_duplicates_merge_groups_and_autocomplete() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let Response::AddressBooks { address_books } =
        ok(&service, Request::ListAddressBooks { source_id: None }).await
    else {
        panic!()
    };
    let book = address_books
        .iter()
        .find(|b| b.role.is_none())
        .unwrap()
        .id
        .clone();
    let mut ids = Vec::new();
    for fields in [
        person("Ada Lovelace", "ada@example.org"),
        person("A. Lovelace", "ADA@example.org"),
        person("Charles Babbage", "charles@example.org"),
    ] {
        let Response::ContactSaved { contact, .. } = ok(
            &service,
            Request::CreateContact {
                address_book_id: book.clone(),
                fields: Box::new(fields),
            },
        )
        .await
        else {
            panic!()
        };
        ids.push(contact.id);
    }
    let Response::ContactDuplicates { groups } = ok(
        &service,
        Request::FindDuplicateContacts {
            address_book_ids: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(groups.len(), 1, "{groups:?}");
    let Response::ContactSaved { contact, .. } = ok(
        &service,
        Request::MergeContacts {
            primary_id: ids[0].clone(),
            contact_ids: vec![ids[1].clone()],
            fields: Box::new(person("Ada Lovelace", "ada@example.org")),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(contact.id, ids[0]);
    let Response::Contacts { total, .. } = ok(
        &service,
        Request::ListContacts {
            address_book_ids: Some(vec![book.clone()]),
            group_id: None,
            offset: 0,
            limit: 50,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(total, 2);
    // Groups.
    let Response::ContactGroupSaved { group, .. } = ok(
        &service,
        Request::CreateContactGroup {
            address_book_id: book.clone(),
            name: "Engines".into(),
            member_ids: vec![ids[0].clone(), ids[2].clone()],
            member_emails: vec!["list@example.org".into()],
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(group.member_ids.len(), 2);
    assert_eq!(group.member_emails, ["list@example.org"]);
    // Search and composer autocomplete find the contact.
    let Response::ContactMatches { matches } = ok(
        &service,
        Request::SearchContacts {
            query: "babb".into(),
            address_book_ids: None,
            limit: 5,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(matches.len(), 1);
    let Response::AddressSuggestions { addresses } = ok(
        &service,
        Request::SuggestAddresses {
            prefix: "char".into(),
            limit: 5,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(addresses[0].email, "charles@example.org", "{addresses:?}");
    // A missing name is refused with its text.
    match service
        .handle(Request::CreateContact {
            address_book_id: book,
            fields: Box::new(ContactFields::named(" ")),
        })
        .await
    {
        Response::Error { localized, .. } => {
            assert_eq!(localized.unwrap().id, "pim-issue-missing-name");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn imports_update_by_uid() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//T//EN\r\n\
BEGIN:VEVENT\r\nUID:imp-1@example.org\r\nDTSTAMP:20261001T000000Z\r\nDTSTART:20261007T090000Z\r\nDTEND:20261007T100000Z\r\nSUMMARY:Imported\r\nX-CUSTOM:kept\r\nEND:VEVENT\r\n\
BEGIN:VTODO\r\nUID:imp-2@example.org\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:Imported task\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    let source = || PimImportSource::Data {
        file_name: Some("events.ics".into()),
        content: ics.into(),
    };
    let Response::PimImportPreview { preview } =
        ok(&service, Request::PreviewPimImport { source: source() }).await
    else {
        panic!()
    };
    assert_eq!((preview.events, preview.tasks), (1, 1));
    let result = import(&service, source()).await;
    assert_eq!((result.created, result.updated), (2, 0));
    let result = import(&service, source()).await;
    assert_eq!((result.created, result.updated), (0, 2));
    let found = instances(&service, "2026-10-07T00:00:00Z", "2026-10-08T00:00:00Z").await;
    assert_eq!(found.len(), 1);
    // A file from disk (launch argument) imports the same way.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("contacts.vcf");
    std::fs::write(
        &path,
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:c-1\r\nFN:Grace Hopper\r\nEMAIL:grace@example.org\r\nEND:VCARD\r\n",
    )
    .unwrap();
    let Response::PimImported { result } = ok(
        &service,
        Request::ImportPim {
            source: PimImportSource::File {
                path: path.display().to_string(),
            },
            calendar_id: None,
            address_book_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.created, 1);
    // Not iCalendar or vCard: refused with a text.
    assert_eq!(
        error(
            &service,
            Request::PreviewPimImport {
                source: PimImportSource::Data {
                    file_name: Some("notes.txt".into()),
                    content: "just text".into(),
                },
            }
        )
        .await,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn calendars_and_address_books_are_managed() {
    let (service, _d, _c) = TempService::quiet().into_parts();
    let Response::CalendarSaved { calendar, op_ids } = ok(
        &service,
        Request::CreateCalendar {
            source_id: "local".into(),
            settings: nail_api::pim::CalendarSettings::named("Sport"),
            components: vec![nail_api::pim::CalendarComponent::Event],
        },
    )
    .await
    else {
        panic!()
    };
    assert!(op_ids.is_empty(), "local writes are not queued");
    let mut settings = calendar.settings.clone();
    settings.color = Some("#22aa66".into());
    settings.visible = false;
    let Response::CalendarSaved { calendar, .. } = ok(
        &service,
        Request::UpdateCalendar {
            calendar_id: calendar.id.clone(),
            settings,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(!calendar.settings.visible);
    ok(
        &service,
        Request::DeleteCalendar {
            calendar_id: calendar.id,
        },
    )
    .await;
    // The Collected book cannot be deleted.
    assert_eq!(
        error(
            &service,
            Request::DeleteAddressBook {
                address_book_id: "local-collected".into(),
            }
        )
        .await,
        ErrorCode::Forbidden
    );
}

async fn mcp_client(
    service: &Service,
    url: &str,
    name: &str,
    grant: nail_api::mcp::McpGrant,
) -> rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientConfig> {
    use rmcp::ServiceExt as _;
    let Response::McpClientCreated { token, .. } = ok(
        service,
        Request::CreateMcpClient {
            name: name.into(),
            grant,
        },
    )
    .await
    else {
        panic!()
    };
    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
        nail_mcp::bridge::loopback_http_client().unwrap(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url)
            .auth_header(token),
    );
    rmcp::model::ClientConfig::default()
        .serve(transport)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_clients_read_calendars_after_a_grant() {
    use nail_api::mcp::{McpGrant, McpScope};
    let (service, _d, _c) = TempService::quiet().into_parts();
    let calendar = default_calendar(&service).await;
    ok(
        &service,
        Request::CreateEvent {
            calendar_id: calendar.clone(),
            fields: Box::new(EventFields::new(
                "Board meeting",
                PimTime::utc("2026-10-07T09:00:00"),
                PimTime::utc("2026-10-07T10:00:00"),
            )),
            recurrence: None,
        },
    )
    .await;
    let running = service.start_mcp_server(0, None).await.unwrap();
    // A grant for every account reaches the local calendars.
    let full = mcp_client(
        &service,
        &running.url(),
        "full",
        McpGrant {
            scopes: vec![McpScope::Read],
            all_accounts: true,
            ..McpGrant::default()
        },
    )
    .await;
    let mut args = serde_json::Map::new();
    args.insert("from".into(), "2026-10-01T00:00:00Z".into());
    args.insert("to".into(), "2026-10-31T00:00:00Z".into());
    let result = full
        .call_tool(
            rmcp::model::CallToolRequestParams::new("list_events").with_arguments(args.clone()),
        )
        .await
        .unwrap();
    let text = serde_json::to_string(&result.structured_content).unwrap();
    assert!(text.contains("Board meeting"), "{text}");
    assert!(
        text.contains("BEGIN:VCALENDAR"),
        "the iCalendar text: {text}"
    );
    let read = full
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "nail://calendars",
        ))
        .await
        .unwrap();
    assert!(
        serde_json::to_string(&read)
            .unwrap()
            .contains(calendar.as_str())
    );
    // A grant for one mail account does not reach local calendars.
    let narrow = mcp_client(
        &service,
        &running.url(),
        "narrow",
        McpGrant {
            scopes: vec![McpScope::Read],
            accounts: vec!["work".into()],
            ..McpGrant::default()
        },
    )
    .await;
    let result = narrow
        .call_tool(rmcp::model::CallToolRequestParams::new("list_events").with_arguments(args))
        .await
        .unwrap();
    let text = serde_json::to_string(&result.structured_content).unwrap();
    assert!(!text.contains("Board meeting"), "{text}");
    // No read grant: refused.
    let none = mcp_client(&service, &running.url(), "none", McpGrant::default()).await;
    let result = none
        .call_tool(rmcp::model::CallToolRequestParams::new("list_tasks"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    drop((
        full.cancel().await,
        narrow.cancel().await,
        none.cancel().await,
    ));
    running.shutdown().await;
}
