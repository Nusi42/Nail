//! Wave M3a (ADRs 0058, 0063): every calendar, contact and task request is
//! served by the core (none answers `unsupported` any more). The flows are
//! tested in `pim.rs` (local data) and `pim_sync.rs` (sources, reminders).

use nail_api::pim::{
    AddressBookSettings, CalendarComponent, CalendarSettings, ConflictChoice, ContactFields,
    EditScope, EventFields, PimImportSource, PimItemRef, PimSourceConfig, PimSourceSettings,
    PimTime, TaskFields,
};
use nail_api::{ErrorCode, Request, Response};
use nail_core::{Service, ServiceOptions};

fn setup() -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    (service, data, config)
}

/// One request of every M3a kind.
fn every_pim_request() -> Vec<Request> {
    let start = PimTime::utc("2026-10-01T09:00:00");
    let event = || {
        Box::new(EventFields::new(
            "Standup",
            start.clone(),
            PimTime::utc("2026-10-01T09:15:00"),
        ))
    };
    let source = || {
        Box::new(PimSourceConfig {
            id: "holidays".into(),
            name: None,
            enabled: true,
            settings: PimSourceSettings::Ics {
                // Loopback (refused at once): tests never reach the network.
                url: "http://127.0.0.1:9/holidays.ics".into(),
                color: None,
                alarms: false,
                refresh_minutes: None,
                auth: None,
            },
        })
    };
    let data = || PimImportSource::Data {
        file_name: None,
        content: "BEGIN:VCARD\r\nEND:VCARD\r\n".into(),
    };
    let contact = || Box::new(ContactFields::named("Ada"));
    let task = || Box::new(TaskFields::titled("Pay rent"));
    vec![
        Request::ListPimSources,
        Request::AddPimSource {
            config: source(),
            secret: None,
        },
        Request::UpdatePimSource {
            config: source(),
            secret: None,
        },
        Request::RemovePimSource {
            source_id: "holidays".into(),
        },
        Request::TestPimSource {
            config: source(),
            secret: None,
        },
        Request::SyncPimSources { source_id: None },
        Request::ListCalendars { source_id: None },
        Request::CreateCalendar {
            source_id: "local".into(),
            settings: CalendarSettings::named("Home"),
            components: vec![CalendarComponent::Event],
        },
        Request::UpdateCalendar {
            calendar_id: "c1".into(),
            settings: CalendarSettings::named("Home"),
        },
        Request::DeleteCalendar {
            calendar_id: "c1".into(),
        },
        Request::ListEventInstances {
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-08T00:00:00Z".into(),
            calendar_ids: None,
            tz: "Europe/Berlin".into(),
        },
        Request::GetEvent {
            event_id: "ev1".into(),
        },
        Request::CreateEvent {
            calendar_id: "c1".into(),
            fields: event(),
            recurrence: None,
        },
        Request::UpdateEvent {
            event_id: "ev1".into(),
            scope: EditScope::This,
            recurrence_id: Some(start.clone()),
            fields: event(),
            recurrence: None,
        },
        Request::DeleteEvent {
            event_id: "ev1".into(),
            scope: EditScope::All,
            recurrence_id: None,
        },
        Request::MoveEvent {
            event_id: "ev1".into(),
            calendar_id: "c2".into(),
        },
        Request::EventFromMessage {
            message_id: "m1".into(),
        },
        Request::FindFreeTime {
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-02T00:00:00Z".into(),
            duration_minutes: 30,
            calendar_ids: None,
            tz: "UTC".into(),
        },
        Request::ListAddressBooks { source_id: None },
        Request::CreateAddressBook {
            source_id: "local".into(),
            settings: AddressBookSettings::named("Friends"),
        },
        Request::UpdateAddressBook {
            address_book_id: "ab1".into(),
            settings: AddressBookSettings::named("Friends"),
        },
        Request::DeleteAddressBook {
            address_book_id: "ab1".into(),
        },
        Request::ListContacts {
            address_book_ids: None,
            group_id: None,
            offset: 0,
            limit: 50,
        },
        Request::GetContact {
            contact_id: "k1".into(),
        },
        Request::CreateContact {
            address_book_id: "ab1".into(),
            fields: contact(),
        },
        Request::UpdateContact {
            contact_id: "k1".into(),
            fields: contact(),
        },
        Request::DeleteContacts {
            contact_ids: vec!["k1".into()],
        },
        Request::MoveContact {
            contact_id: "k1".into(),
            address_book_id: "ab2".into(),
        },
        Request::SearchContacts {
            query: "ada".into(),
            address_book_ids: None,
            limit: 10,
        },
        Request::FindDuplicateContacts {
            address_book_ids: None,
        },
        Request::MergeContacts {
            primary_id: "k1".into(),
            contact_ids: vec!["k2".into()],
            fields: contact(),
        },
        Request::UnlinkContact {
            contact_id: "k2".into(),
        },
        Request::GetContactPhoto {
            contact_id: "k1".into(),
        },
        Request::ListContactGroups {
            address_book_ids: None,
        },
        Request::CreateContactGroup {
            address_book_id: "ab1".into(),
            name: "Team".into(),
            member_ids: vec![],
            member_emails: vec![],
        },
        Request::UpdateContactGroup {
            group_id: "g1".into(),
            name: "Team".into(),
            member_ids: vec![],
            member_emails: vec![],
        },
        Request::DeleteContactGroup {
            group_id: "g1".into(),
        },
        Request::ListTasks {
            calendar_ids: None,
            include_completed: false,
        },
        Request::GetTask {
            task_id: "t1".into(),
        },
        Request::CreateTask {
            calendar_id: "c1".into(),
            fields: task(),
            recurrence: None,
        },
        Request::UpdateTask {
            task_id: "t1".into(),
            fields: task(),
            recurrence: None,
        },
        Request::CompleteTask {
            task_id: "t1".into(),
            completed: true,
        },
        Request::MoveTask {
            task_id: "t1".into(),
            calendar_id: "c2".into(),
        },
        Request::DeleteTask {
            task_id: "t1".into(),
            with_subtasks: true,
        },
        Request::TaskFromMessage {
            message_id: "m1".into(),
        },
        Request::SearchPim {
            query: "standup".into(),
            kinds: vec![],
            limit: 10,
        },
        Request::PreviewPimImport { source: data() },
        Request::ImportPim {
            source: data(),
            calendar_id: None,
            address_book_id: None,
        },
        Request::ResolvePimConflict {
            item: PimItemRef::Task {
                task_id: "t1".into(),
            },
            keep: ConflictChoice::Server,
        },
        Request::ListReminders { until: None },
        Request::SnoozeReminders {
            reminder_ids: vec!["r1".into()],
            until: "2026-10-01T09:10:00Z".into(),
        },
        Request::DismissReminders {
            reminder_ids: vec!["r1".into()],
        },
    ]
}

#[tokio::test]
async fn every_pim_request_is_served() {
    let (service, _data, _config) = setup();
    let requests = every_pim_request();
    assert_eq!(requests.len(), 52);
    for request in requests {
        let name = serde_json::to_value(&request).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned();
        if let Response::Error { code, message, .. } = service.handle(request).await {
            // Unknown ids and invalid data are fine here; `unsupported` or
            // an internal error is not.
            assert!(
                !matches!(code, ErrorCode::Unsupported | ErrorCode::Internal),
                "{name}: {code:?} {message}"
            );
        }
    }
}
