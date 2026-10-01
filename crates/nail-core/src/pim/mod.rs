//! Calendars, contacts and tasks (M3a; contract ADR 0058, integration
//! ADR 0063).
//!
//! The core owns the PIM data like mail: the store (`nail-store`, ADR
//! 0061) holds every item twice — parsed (`nail_api::pim` types) and as
//! the raw iCalendar / vCard text it came in (`nail-ical`, ADR 0059,
//! patches that text on every write so unknown properties survive) —
//! and every write to a server goes through the PIM operation queue.
//!
//! * [`sources`] — `ListPimSources`, adding / changing / removing / testing
//!   sources in `accounts.kdl`, calendars and address books, the built-in
//!   local source and its default collections.
//! * [`engine`] — one sync engine per enabled subscription or DAV source:
//!   ICS polling (conditional GET), CalDAV / CardDAV discovery and
//!   `sync-collection` (`nail-dav`, ADR 0060), replay of the PIM queue,
//!   backoff while offline.
//! * [`events`] — events (edit scopes), instances, free time; tasks.
//! * [`contacts`] — contacts, groups, duplicates, merging, photos.
//! * [`import`] — `.ics` / `.vcf` imports and "create event / task from
//!   email".
//! * [`reminders`] — the reminder scheduler (`nail-triage`) on the store,
//!   its task, snooze / dismiss, notifications.
//! * [`search`] — the PIM search index (in memory, rebuilt from the store).

mod contacts;
mod engine;
mod events;
mod import;
mod reminders;
mod search;
mod sources;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use nail_api::pim::{CalendarEvent, EventFields, PimIssue, PimTime, Recurrence, TaskFields};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, LocalizedMessage, OpId, PimSourceId, Request, Response,
};
use nail_store::pim::{ExpandedInstance, InstanceExpander, PimOpId};
use nail_store::{BlobHash, BlobStore};
use tokio::sync::Notify;

use crate::error::{CoreError, Result};
use crate::service::Service;

pub(crate) use engine::SourceHandle;

/// PIM state of a running core.
#[derive(Default)]
pub(crate) struct PimCtl {
    /// The reminder scheduler (loaded on first use).
    reminders: Mutex<Option<reminders::Scheduler>>,
    /// Wakes the reminder task (plans or snoozes changed).
    pub(crate) reminder_wake: Notify,
    /// One sync engine per enabled subscription / DAV source.
    engines: Mutex<HashMap<PimSourceId, SourceHandle>>,
    /// The PIM search index (built from the store on first use).
    index: Mutex<Option<Arc<nail_search::PimIndex>>>,
    /// The local source's default collections exist.
    local_ready: std::sync::atomic::AtomicBool,
    /// The source configurations last reconciled (changes are announced).
    last_sources: Mutex<Vec<nail_api::pim::PimSourceConfig>>,
}

/// Answers one PIM request.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    service.ensure_local_pim().await?;
    match request {
        Request::ListPimSources
        | Request::AddPimSource { .. }
        | Request::UpdatePimSource { .. }
        | Request::RemovePimSource { .. }
        | Request::TestPimSource { .. }
        | Request::SyncPimSources { .. }
        | Request::ListCalendars { .. }
        | Request::CreateCalendar { .. }
        | Request::UpdateCalendar { .. }
        | Request::DeleteCalendar { .. }
        | Request::ListAddressBooks { .. }
        | Request::CreateAddressBook { .. }
        | Request::UpdateAddressBook { .. }
        | Request::DeleteAddressBook { .. }
        | Request::ResolvePimConflict { .. } => sources::handle(service, request).await,
        Request::ListEventInstances { .. }
        | Request::GetEvent { .. }
        | Request::CreateEvent { .. }
        | Request::UpdateEvent { .. }
        | Request::DeleteEvent { .. }
        | Request::MoveEvent { .. }
        | Request::FindFreeTime { .. }
        | Request::ListTasks { .. }
        | Request::GetTask { .. }
        | Request::CreateTask { .. }
        | Request::UpdateTask { .. }
        | Request::CompleteTask { .. }
        | Request::MoveTask { .. }
        | Request::DeleteTask { .. } => events::handle(service, request).await,
        Request::ListContacts { .. }
        | Request::GetContact { .. }
        | Request::CreateContact { .. }
        | Request::UpdateContact { .. }
        | Request::DeleteContacts { .. }
        | Request::MoveContact { .. }
        | Request::SearchContacts { .. }
        | Request::FindDuplicateContacts { .. }
        | Request::MergeContacts { .. }
        | Request::UnlinkContact { .. }
        | Request::GetContactPhoto { .. }
        | Request::ListContactGroups { .. }
        | Request::CreateContactGroup { .. }
        | Request::UpdateContactGroup { .. }
        | Request::DeleteContactGroup { .. } => contacts::handle(service, request).await,
        Request::SearchPim {
            query,
            kinds,
            limit,
        } => Ok(Response::PimSearchResults {
            hits: service.search_pim(query, kinds, limit).await?,
        }),
        Request::EventFromMessage { .. }
        | Request::TaskFromMessage { .. }
        | Request::PreviewPimImport { .. }
        | Request::ImportPim { .. } => import::handle(service, request).await,
        Request::ListReminders { .. }
        | Request::SnoozeReminders { .. }
        | Request::DismissReminders { .. } => reminders::handle(service, request).await,
        other => Err(ApiError::internal(format!(
            "{} routed to the PIM module",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// Starts the PIM background work (called by [`Service::start`]): the
/// local defaults, the zone-data check of the instance cache, the source
/// engines and the reminder task.
pub(crate) fn start(service: &Service) -> Vec<tokio::task::JoinHandle<()>> {
    let mut tasks = Vec::new();
    let s = service.clone();
    tasks.push(tokio::spawn(async move {
        if let Err(err) = s.ensure_local_pim().await {
            tracing::warn!(%err, "cannot prepare the local calendars and address books");
        }
        let version = chrono_tz::IANA_TZDB_VERSION;
        match s
            .inner
            .store
            .call(move |st| st.note_zone_data(version))
            .await
        {
            Ok(true) => tracing::info!(version, "zone data changed: instance cache dropped"),
            Ok(false) => {}
            Err(err) => tracing::warn!(%err, "cannot check the zone data of the instance cache"),
        }
        s.reconcile_pim_sources().await;
    }));
    tasks.push(tokio::spawn(reminders::run(service.clone())));
    tasks
}

/// Stops every source engine (shutdown).
pub(crate) fn stop(service: &Service) {
    let engines: Vec<SourceHandle> = crate::service::lock(&service.inner.pim.engines)
        .drain()
        .map(|(_, h)| h)
        .collect();
    for engine in engines {
        engine.stop();
    }
}

// ---------------------------------------------------------------------------
// Shared helpers

/// `invalid_request` for validation issues, with the first one's text.
pub(crate) fn invalid(issues: &[PimIssue]) -> CoreError {
    let ids: Vec<&str> = issues.iter().map(|i| i.message_id()).collect();
    let mut error = ApiError::invalid_request(format!("invalid PIM data: {}", ids.join(", ")));
    if let Some(first) = issues.first() {
        error = error.with_localized(LocalizedMessage::new(first.message_id()));
    }
    error.into()
}

/// `forbidden` (read-only calendars, subscriptions, the Collected book).
pub(crate) fn forbidden(message: impl Into<String>) -> CoreError {
    ApiError::new(ErrorCode::Forbidden, message).into()
}

/// `not_found`.
pub(crate) fn not_found(what: impl std::fmt::Display) -> CoreError {
    ApiError::not_found(format!("{what} not found")).into()
}

/// Zones named in times must exist (the contract only checks the shape).
fn zone_issue(time: &PimTime) -> Option<PimIssue> {
    match time {
        PimTime::Zoned { tz, .. } if nail_ical::time::zone(tz).is_none() => {
            Some(PimIssue::InvalidTimeZone)
        }
        _ => None,
    }
}

/// Every problem of an event's fields and recurrence, zones included.
pub(crate) fn event_issues(fields: &EventFields, recurrence: Option<&Recurrence>) -> Vec<PimIssue> {
    let mut out = fields.issues();
    let mut extra: Vec<PimIssue> = [&fields.start, &fields.end]
        .into_iter()
        .filter_map(zone_issue)
        .collect();
    if let Some(recurrence) = recurrence {
        extra.extend(recurrence.issues(&fields.start));
    }
    // Different zones: compare the instants.
    if !out.contains(&PimIssue::MixedAllDay)
        && let (Some(start), Some(end)) = (
            nail_ical::time::instant(&fields.start, Tz::UTC),
            nail_ical::time::instant(&fields.end, Tz::UTC),
        )
        && end < start
    {
        extra.push(PimIssue::EndBeforeStart);
    }
    for issue in extra {
        if !out.contains(&issue) {
            out.push(issue);
        }
    }
    out
}

/// Every problem of a task's fields and recurrence, zones included.
pub(crate) fn task_issues(fields: &TaskFields, recurrence: Option<&Recurrence>) -> Vec<PimIssue> {
    let mut out = fields.issues();
    for time in fields.start.iter().chain(&fields.due) {
        if let Some(issue) = zone_issue(time)
            && !out.contains(&issue)
        {
            out.push(issue);
        }
    }
    if let (Some(recurrence), Some(anchor)) =
        (recurrence, fields.start.as_ref().or(fields.due.as_ref()))
    {
        for issue in recurrence.issues(anchor) {
            if !out.contains(&issue) {
                out.push(issue);
            }
        }
    }
    out
}

/// A new random id with a prefix (`ev-…`, `task-…`, `ct-…`).
pub(crate) fn new_id(prefix: &str) -> String {
    format!(
        "{prefix}-{:016x}{:08x}",
        rand::random::<u64>(),
        rand::random::<u32>()
    )
}

/// A new random UUID (version 4), lower case.
pub(crate) fn new_uuid() -> String {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// A deterministic id of an item a server sent (`<prefix>-<hash>`), so a
/// resync finds the same rows.
pub(crate) fn derived_id(prefix: &str, collection: &str, uid: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(collection.as_bytes());
    hash.update([0]);
    hash.update(uid.as_bytes());
    format!("{prefix}-{}", &hex::encode(hash.finalize())[..20])
}

/// Contract operation ids of queued PIM operations.
pub(crate) fn op_ids(ids: &[PimOpId]) -> Vec<OpId> {
    ids.iter().map(|id| OpId::new(id.to_string())).collect()
}

/// Parses an RFC 3339 instant to Unix seconds.
pub(crate) fn parse_instant(text: &str) -> Result<i64> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.timestamp())
        .map_err(|_| ApiError::invalid_request(format!("invalid time {text:?}")).into())
}

/// Unix seconds as a UTC date-time.
pub(crate) fn utc(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap_or_default()
}

/// An IANA zone (or `UTC`) the request names; `invalid_request` else.
pub(crate) fn request_zone(name: &str) -> Result<Tz> {
    nail_ical::time::zone(name).ok_or_else(|| {
        CoreError::from(
            ApiError::invalid_request(format!("unknown time zone {name:?}")).with_localized(
                LocalizedMessage::new(PimIssue::InvalidTimeZone.message_id()),
            ),
        )
    })
}

/// The local zone of this machine (IANA), `UTC` when unknown; `NAIL_TZ`
/// overrides it (tests, demos).
pub(crate) fn local_zone_name() -> String {
    std::env::var("NAIL_TZ")
        .ok()
        .filter(|z| nail_ical::time::zone(z).is_some())
        .or_else(|| {
            iana_time_zone::get_timezone()
                .ok()
                .filter(|z| nail_ical::time::zone(z).is_some())
        })
        .unwrap_or_else(|| "UTC".to_owned())
}

/// The local zone of this machine.
pub(crate) fn local_zone() -> Tz {
    nail_ical::time::zone(&local_zone_name()).unwrap_or(Tz::UTC)
}

/// Stored bytes by SHA-256 (the writers' blob lookup).
pub(crate) fn blob_bytes(blobs: &BlobStore, sha256: &str) -> Option<Vec<u8>> {
    let hash = BlobHash::parse(sha256).ok()?;
    blobs.get(&hash).ok()
}

/// The instance cache's expander: `nail-ical`'s recurrence engine in the
/// cache's frame (zoned times as instants, dates and floating times as
/// wall clock read as UTC).
pub(crate) struct Expander;

impl InstanceExpander for Expander {
    fn expand(&self, event: &CalendarEvent, from: i64, until: i64) -> Vec<ExpandedInstance> {
        let options = nail_ical::expand::ExpandOptions {
            from: utc(from),
            to: utc(until),
            viewer: Tz::UTC,
            limit: 50_000,
            this_and_future: &[],
        };
        nail_ical::expand::instances(event, &options)
            .into_iter()
            .map(|i| ExpandedInstance {
                recurrence_id: i.recurrence_id,
                start: i.start,
                end: i.end,
                overridden: i.overridden,
                cancelled: false,
            })
            .collect()
    }
}

impl Service {
    /// Now for the PIM logic (the triage clock: tests and demos move it).
    pub(crate) fn pim_now(&self) -> i64 {
        self.inner.clock.unix()
    }

    /// Now as a date-time (writers' `DTSTAMP` / `REV`).
    pub(crate) fn pim_utc_now(&self) -> DateTime<Utc> {
        self.inner.clock.now()
    }

    /// The mail account a source belongs to (DAV sources with `account=`).
    pub(crate) fn pim_source_account(&self, source: &PimSourceId) -> Option<AccountId> {
        self.inner
            .accounts
            .config()
            .pim_source(source)
            .and_then(|s| s.account().cloned())
    }

    /// Broadcasts PIM change events.
    pub(crate) fn pim_events(&self, events: impl IntoIterator<Item = Event>) {
        for event in events {
            self.broadcast(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::{EventFields, Recurrence, RecurrenceRule};

    #[test]
    fn ids_and_uuids() {
        let a = new_id("ev");
        assert!(a.starts_with("ev-") && a.len() == 3 + 24, "{a}");
        assert_ne!(a, new_id("ev"));
        let uuid = new_uuid();
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[14..15], "4");
        assert_eq!(derived_id("ev", "c", "u"), derived_id("ev", "c", "u"));
        assert_ne!(derived_id("ev", "c", "u"), derived_id("ev", "c2", "u"));
    }

    #[test]
    fn event_issues_check_zones_and_instants() {
        let mut fields = EventFields::new(
            "Call",
            PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin"),
            PimTime::utc("2026-10-01T07:30:00"),
        );
        assert!(event_issues(&fields, None).is_empty());
        fields.end = PimTime::utc("2026-10-01T06:30:00");
        assert_eq!(event_issues(&fields, None), [PimIssue::EndBeforeStart]);
        fields.end = PimTime::zoned("2026-10-01T10:00:00", "Mars/Olympus");
        assert_eq!(event_issues(&fields, None), [PimIssue::InvalidTimeZone]);
        let fields = EventFields::new(
            "Day",
            PimTime::date("2026-10-01"),
            PimTime::date("2026-10-02"),
        );
        let mut rule = RecurrenceRule::new(nail_api::pim::calendar::Frequency::Daily);
        rule.interval = 0;
        assert_eq!(
            event_issues(&fields, Some(&Recurrence::rule(rule))),
            [PimIssue::InvalidRecurrence]
        );
    }

    #[test]
    fn invalid_carries_the_first_issue() {
        let err: ApiError = invalid(&[PimIssue::MissingName, PimIssue::InvalidEmail]).into();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert_eq!(
            err.localized.unwrap().id,
            PimIssue::MissingName.message_id()
        );
    }

    #[test]
    fn expander_works_in_cache_seconds() {
        let mut fields = EventFields::new(
            "Floating",
            PimTime::floating("2026-10-05T09:00:00"),
            PimTime::floating("2026-10-05T10:00:00"),
        );
        fields.title = "Standup".into();
        let event = CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            fields,
            recurrence: Some(Recurrence::rule(RecurrenceRule::new(
                nail_api::pim::calendar::Frequency::Daily,
            ))),
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: nail_api::pim::PimItemSync::Synced,
        };
        let from = Utc
            .with_ymd_and_hms(2026, 10, 6, 0, 0, 0)
            .unwrap()
            .timestamp();
        let list = Expander.expand(&event, from, from + 2 * 86_400);
        let starts: Vec<&PimTime> = list.iter().map(|i| &i.start).collect();
        assert_eq!(
            starts,
            [
                &PimTime::floating("2026-10-06T09:00:00"),
                &PimTime::floating("2026-10-07T09:00:00")
            ]
        );
    }
}
