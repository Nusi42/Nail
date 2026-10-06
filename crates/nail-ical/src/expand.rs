//! A series → its [`EventInstance`]s in a range, for calendar views and
//! the instance cache.
//!
//! * Occurrences come from [`crate::recur::Occurrences`] in the start's
//!   wall-clock frame; each becomes an instant with RFC 5545's gap / fold
//!   rules. All-day and floating events are placed in the viewer's zone.
//! * The length of an occurrence is the master's **wall-clock** length
//!   when start and end share a zone (a 9:00–10:00 meeting stays
//!   9:00–10:00 across DST changes), else its exact length.
//! * Overrides (`RECURRENCE-ID`) replace their occurrence, wherever they
//!   moved it; an override whose occurrence is excluded (`EXDATE`) or
//!   that is cancelled is hidden.
//! * `RANGE=THISANDFUTURE` overrides apply to every later occurrence:
//!   their fields, shifted by the override's move.
//! * Cost: iteration jumps ahead to the range
//!   ([`crate::recur::Occurrences::skip_before`]) instead of walking from
//!   the series start, scans at most [`MAX_SCANNED`] occurrences per
//!   series, and a [`ScanBudget`] shared by every series of one request
//!   bounds the whole request (ADR 0064).

use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use chrono_tz::Tz;
use nail_api::pim::calendar::EventStatus;
use nail_api::pim::{
    CalendarEvent, EventFields, EventInstance, EventOverride, PimTime, Recurrence,
};

use crate::recur::Occurrences;
use crate::time;

/// Most occurrences looked at for one series and range (guards against
/// `SECONDLY` rules over long ranges).
pub const MAX_SCANNED: usize = 200_000;

/// How many occurrences one request (a calendar view, a reminder plan)
/// may scan over all its series; see [`ScanBudget`].
pub const REQUEST_SCAN_BUDGET: usize = 1_000_000;

/// Occurrences that may still be scanned, shared by every expansion of a
/// request: a calendar with thousands of hostile series costs one budget,
/// not thousands of [`MAX_SCANNED`]s. Once spent, expansions stop early
/// (their results are incomplete; [`ScanBudget::exhausted`] says so).
#[derive(Debug)]
pub struct ScanBudget {
    left: AtomicUsize,
    spent_out: std::sync::atomic::AtomicBool,
}

impl ScanBudget {
    /// A budget of `scans` occurrences.
    pub fn new(scans: usize) -> Self {
        Self {
            left: AtomicUsize::new(scans),
            spent_out: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Takes one scan; `false` when the budget is spent.
    pub fn take(&self) -> bool {
        // A compare-exchange loop (`fetch_update` is deprecated since Rust
        // 1.99, its replacement `try_update` is newer than `rust-version`).
        let mut current = self.left.load(Ordering::Relaxed);
        let taken = loop {
            let Some(next) = current.checked_sub(1) else {
                break false;
            };
            match self.left.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break true,
                Err(actual) => current = actual,
            }
        };
        if !taken {
            self.spent_out.store(true, Ordering::Relaxed);
        }
        taken
    }

    /// Whether an expansion was cut short because the budget ran out.
    pub fn exhausted(&self) -> bool {
        self.spent_out.load(Ordering::Relaxed)
    }
}

impl Default for ScanBudget {
    fn default() -> Self {
        Self::new(REQUEST_SCAN_BUDGET)
    }
}

/// The length of an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// Wall-clock length (start and end in the same zone / kind).
    Wall(Duration),
    /// Exact length (start and end in different zones).
    Exact(Duration),
}

impl Span {
    /// The span from `start` to `end`.
    pub fn between(start: &PimTime, end: &PimTime, viewer: Tz) -> Self {
        let same_frame =
            std::mem::discriminant(start) == std::mem::discriminant(end) && start.tz() == end.tz();
        if same_frame && let (Some(s), Some(e)) = (time::wall(start), time::wall(end)) {
            return Self::Wall(e - s);
        }
        match (time::instant(start, viewer), time::instant(end, viewer)) {
            (Some(s), Some(e)) => Self::Exact(e - s),
            _ => Self::Wall(Duration::zero()),
        }
    }

    /// The end of an occurrence starting at `start`, expressed like
    /// `end_like` (the master's end).
    pub fn end(&self, start: &PimTime, end_like: &PimTime, viewer: Tz) -> PimTime {
        match *self {
            Self::Wall(d) => match time::wall(start) {
                Some(w) => time::like(w + d, start),
                None => start.clone(),
            },
            Self::Exact(d) => {
                let Some(instant) = time::instant(start, viewer).map(|i| i + d) else {
                    return start.clone();
                };
                let zone = end_like.tz().and_then(time::zone).unwrap_or(viewer);
                time::like(time::to_local(instant, zone), end_like)
            }
        }
    }
}

/// What to expand.
#[derive(Debug, Clone, Copy)]
pub struct ExpandOptions<'a> {
    /// Range start (inclusive).
    pub from: DateTime<Utc>,
    /// Range end (exclusive).
    pub to: DateTime<Utc>,
    /// The viewer's zone: places all-day and floating events.
    pub viewer: Tz,
    /// Most instances returned.
    pub limit: usize,
    /// Recurrence ids of `RANGE=THISANDFUTURE` overrides
    /// ([`crate::ParsedEvent::this_and_future`]); overrides with
    /// [`EventOverride::this_and_future`] set count as well.
    pub this_and_future: &'a [PimTime],
    /// The request's budget (`None`: only [`MAX_SCANNED`] per series).
    pub budget: Option<&'a ScanBudget>,
}

impl ExpandOptions<'_> {
    /// A range in a zone, without `THISANDFUTURE` overrides.
    pub fn range(from: DateTime<Utc>, to: DateTime<Utc>, viewer: Tz) -> Self {
        ExpandOptions {
            from,
            to,
            viewer,
            limit: 10_000,
            this_and_future: &[],
            budget: None,
        }
    }
}

fn instance(
    event: &CalendarEvent,
    fields: &EventFields,
    recurrence_id: Option<PimTime>,
    start: PimTime,
    end: PimTime,
    overridden: bool,
    viewer: Tz,
) -> Option<EventInstance> {
    let start_utc = time::instant(&start, viewer)?;
    let end_utc = time::instant(&end, viewer)?.max(start_utc);
    Some(EventInstance {
        event_id: event.id.clone(),
        calendar_id: event.calendar_id.clone(),
        recurrence_id,
        all_day: start.is_date(),
        start,
        end,
        start_utc: time::format_instant(start_utc),
        end_utc: time::format_instant(end_utc),
        title: fields.title.clone(),
        location: fields.location.clone(),
        status: fields.status,
        transparency: fields.transparency,
        color: fields.color.clone(),
        recurring: event.is_recurring(),
        overridden,
        has_attendees: !fields.attendees.is_empty(),
        has_alarms: !fields.alarms.is_empty(),
        read_only: event.read_only,
    })
}

fn overlaps(i: &EventInstance, options: &ExpandOptions<'_>) -> bool {
    let (Some(start), Some(end)) = (
        time::parse_instant(&i.start_utc),
        time::parse_instant(&i.end_utc),
    ) else {
        return false;
    };
    if start == end {
        start >= options.from && start < options.to
    } else {
        start < options.to && end > options.from
    }
}

/// The instances of an event (series) that overlap the range, sorted by
/// start; cancelled ones are left out.
pub fn instances(event: &CalendarEvent, options: &ExpandOptions<'_>) -> Vec<EventInstance> {
    let viewer = options.viewer;
    let master = &event.fields;
    let mut out = Vec::new();
    let recurrence: Option<&Recurrence> = event.recurrence.as_ref().filter(|r| !r.is_empty());
    let Some(recurrence) = recurrence else {
        if master.status != EventStatus::Cancelled
            && let Some(i) = instance(
                event,
                master,
                None,
                master.start.clone(),
                master.end.clone(),
                false,
                viewer,
            )
            && overlaps(&i, options)
        {
            out.push(i);
        }
        return out;
    };
    let start = &master.start;
    let Some(mut occurrences) = Occurrences::new(start, Some(recurrence)) else {
        return out;
    };
    let span = Span::between(&master.start, &master.end, viewer);
    let frame = |t: &PimTime| time::to_frame(t, start);
    let exact: Vec<(NaiveDateTime, &EventOverride)> = event
        .overrides
        .iter()
        .filter_map(|o| Some((frame(&o.recurrence_id)?, o)))
        .collect();
    let mut future: Vec<(NaiveDateTime, &EventOverride)> = exact
        .iter()
        .filter(|(_, o)| o.this_and_future || options.this_and_future.contains(&o.recurrence_id))
        .copied()
        .collect();
    future.sort_by_key(|(f, _)| *f);
    // How far THISANDFUTURE overrides move occurrences earlier.
    let earliest_shift = future
        .iter()
        .filter_map(|(rid, o)| Some(frame(&o.fields.start)? - *rid))
        .min()
        .unwrap_or_else(Duration::zero)
        .min(Duration::zero());
    let horizon = options.to + Duration::days(2) - earliest_shift;

    // Exact overrides, wherever they moved their occurrence.
    for (rid, o) in &exact {
        if occurrences.is_excluded(*rid) || o.fields.status == EventStatus::Cancelled {
            continue;
        }
        if let Some(i) = instance(
            event,
            &o.fields,
            Some(time::like(*rid, start)),
            o.fields.start.clone(),
            o.fields.end.clone(),
            true,
            viewer,
        ) && overlaps(&i, options)
        {
            out.push(i);
        }
    }
    let longest = match span {
        Span::Wall(d) | Span::Exact(d) => d.max(Duration::zero()),
    };
    let earliest_start = options.from - longest - Duration::days(2);
    // Jump ahead to the range: occurrences before `earliest_start` matter
    // only when a THISANDFUTURE override moves them later.
    let latest_shift = future
        .iter()
        .filter_map(|(rid, o)| Some(frame(&o.fields.start)? - *rid))
        .max()
        .unwrap_or_else(Duration::zero)
        .max(Duration::zero());
    let frame_zone = start.tz().and_then(time::zone).unwrap_or(viewer);
    if let Some(skip_to) = earliest_start.checked_sub_signed(latest_shift + Duration::days(2)) {
        occurrences.skip_before(time::to_local(skip_to, frame_zone));
    }
    for (scanned, occurrence) in occurrences.by_ref().enumerate() {
        if scanned >= MAX_SCANNED || options.budget.is_some_and(|b| !b.take()) {
            break;
        }
        let occurrence_start = time::like(occurrence, start);
        let Some(at) = time::instant(&occurrence_start, viewer) else {
            continue;
        };
        if at > horizon {
            break;
        }
        if exact.iter().any(|(rid, _)| *rid == occurrence) {
            continue;
        }
        let ruling = future.iter().rev().find(|(rid, _)| *rid <= occurrence);
        let (fields, istart, iend, overridden) = if let Some((rid, o)) = ruling {
            let shift = frame(&o.fields.start).map_or_else(Duration::zero, |s| s - *rid);
            let istart = time::like(occurrence + shift, start);
            let ospan = Span::between(&o.fields.start, &o.fields.end, viewer);
            let iend = ospan.end(&istart, &o.fields.end, viewer);
            (&o.fields, istart, iend, true)
        } else {
            if at < earliest_start {
                continue;
            }
            let iend = span.end(&occurrence_start, &master.end, viewer);
            (master, occurrence_start.clone(), iend, false)
        };
        if fields.status == EventStatus::Cancelled {
            continue;
        }
        if let Some(i) = instance(
            event,
            fields,
            Some(occurrence_start),
            istart,
            iend,
            overridden,
            viewer,
        ) && overlaps(&i, options)
        {
            out.push(i);
        }
    }
    out.sort_by(|a, b| {
        a.start_utc
            .cmp(&b.start_utc)
            .then_with(|| a.title.cmp(&b.title))
    });
    out.truncate(options.limit);
    out
}

/// The occurrence starts (as stored kinds) of a series up to `until`
/// (inclusive), at most `limit`; for tasks and reminders.
pub fn occurrence_starts(
    start: &PimTime,
    recurrence: Option<&Recurrence>,
    until: DateTime<Utc>,
    viewer: Tz,
    limit: usize,
) -> Vec<PimTime> {
    let Some(occurrences) = Occurrences::new(start, recurrence) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for occurrence in occurrences.take(MAX_SCANNED) {
        let t = time::like(occurrence, start);
        if time::instant(&t, viewer).is_none_or(|i| i > until) || out.len() >= limit {
            break;
        }
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::calendar::Frequency;
    use nail_api::pim::source::PimItemSync;
    use nail_api::pim::{RecurrenceEnd, RecurrenceRule};
    use pretty_assertions::assert_eq;

    fn utc(text: &str) -> DateTime<Utc> {
        time::parse_instant(text).unwrap()
    }

    fn series(start: PimTime, end: PimTime, rule: &str) -> CalendarEvent {
        CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            recurrence: Some(Recurrence::rule(
                crate::recur::parse_rrule(rule, &start).unwrap(),
            )),
            fields: EventFields::new("Standup", start, end),
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    fn starts(list: &[EventInstance]) -> Vec<&str> {
        list.iter().map(|i| i.start_utc.as_str()).collect()
    }

    #[test]
    fn keeps_local_time_across_dst() {
        let event = series(
            PimTime::zoned("2026-10-23T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-23T09:15:00", "Europe/Berlin"),
            "FREQ=DAILY;COUNT=4",
        );
        let list = instances(
            &event,
            &ExpandOptions::range(
                utc("2026-10-01T00:00:00Z"),
                utc("2026-11-01T00:00:00Z"),
                Tz::UTC,
            ),
        );
        assert_eq!(
            starts(&list),
            [
                "2026-10-23T07:00:00Z",
                "2026-10-24T07:00:00Z",
                "2026-10-25T08:00:00Z",
                "2026-10-26T08:00:00Z"
            ]
        );
        assert_eq!(list[2].end_utc, "2026-10-25T08:15:00Z");
        assert_eq!(
            list[2].recurrence_id,
            Some(PimTime::zoned("2026-10-25T09:00:00", "Europe/Berlin"))
        );
        assert!(list.iter().all(|i| i.recurring && !i.overridden));
    }

    #[test]
    fn occurrences_in_dst_gaps_and_folds() {
        // 02:30 daily in Berlin: the gap day moves to 03:30 CEST, the fold
        // day takes the first 02:30.
        let event = series(
            PimTime::zoned("2026-03-28T02:30:00", "Europe/Berlin"),
            PimTime::zoned("2026-03-28T03:00:00", "Europe/Berlin"),
            "FREQ=DAILY;COUNT=3",
        );
        let list = instances(
            &event,
            &ExpandOptions::range(
                utc("2026-03-01T00:00:00Z"),
                utc("2026-04-01T00:00:00Z"),
                Tz::UTC,
            ),
        );
        assert_eq!(
            starts(&list),
            [
                "2026-03-28T01:30:00Z",
                "2026-03-29T01:30:00Z",
                "2026-03-30T00:30:00Z"
            ]
        );
        let fold = series(
            PimTime::zoned("2026-10-25T02:30:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-25T03:30:00", "Europe/Berlin"),
            "FREQ=YEARLY;COUNT=1",
        );
        let list = instances(
            &fold,
            &ExpandOptions::range(
                utc("2026-10-01T00:00:00Z"),
                utc("2026-11-01T00:00:00Z"),
                Tz::UTC,
            ),
        );
        assert_eq!(list[0].start_utc, "2026-10-25T00:30:00Z");
        assert_eq!(
            list[0].end_utc, "2026-10-25T02:30:00Z",
            "wall-clock hour across the fold"
        );
    }

    #[test]
    fn all_day_and_floating_follow_the_viewer() {
        let event = series(
            PimTime::date("2026-12-24"),
            PimTime::date("2026-12-27"),
            "FREQ=YEARLY",
        );
        let tokyo = chrono_tz::Asia::Tokyo;
        let list = instances(
            &event,
            &ExpandOptions::range(
                utc("2026-12-01T00:00:00Z"),
                utc("2028-01-01T00:00:00Z"),
                tokyo,
            ),
        );
        assert_eq!(
            starts(&list),
            ["2026-12-23T15:00:00Z", "2027-12-23T15:00:00Z"]
        );
        assert_eq!(list[0].end, PimTime::date("2026-12-27"));
        assert!(list[0].all_day);
        let floating = series(
            PimTime::floating("2026-10-05T09:00:00"),
            PimTime::floating("2026-10-05T10:00:00"),
            "FREQ=DAILY;COUNT=2",
        );
        let ny = chrono_tz::America::New_York;
        let list = instances(
            &floating,
            &ExpandOptions::range(utc("2026-10-01T00:00:00Z"), utc("2026-11-01T00:00:00Z"), ny),
        );
        assert_eq!(
            starts(&list),
            ["2026-10-05T13:00:00Z", "2026-10-06T13:00:00Z"]
        );
    }

    #[test]
    fn ranges_cut_and_overlap() {
        let event = series(
            PimTime::utc("2026-10-01T23:00:00"),
            PimTime::utc("2026-10-02T01:00:00"),
            "FREQ=DAILY",
        );
        let list = instances(
            &event,
            &ExpandOptions::range(
                utc("2026-10-05T00:00:00Z"),
                utc("2026-10-07T00:00:00Z"),
                Tz::UTC,
            ),
        );
        // The one from the 4th overlaps the start of the range.
        assert_eq!(
            starts(&list),
            [
                "2026-10-04T23:00:00Z",
                "2026-10-05T23:00:00Z",
                "2026-10-06T23:00:00Z"
            ]
        );
        let mut limited = ExpandOptions::range(
            utc("2026-10-05T00:00:00Z"),
            utc("2027-10-07T00:00:00Z"),
            Tz::UTC,
        );
        limited.limit = 5;
        assert_eq!(instances(&event, &limited).len(), 5);
        // Zero-length events count when they start inside.
        let point = series(
            PimTime::utc("2026-10-01T10:00:00"),
            PimTime::utc("2026-10-01T10:00:00"),
            "FREQ=DAILY;COUNT=3",
        );
        let list = instances(
            &point,
            &ExpandOptions::range(
                utc("2026-10-02T10:00:00Z"),
                utc("2026-10-03T10:00:00Z"),
                Tz::UTC,
            ),
        );
        assert_eq!(starts(&list), ["2026-10-02T10:00:00Z"]);
    }

    #[test]
    fn overrides_exceptions_and_this_and_future() {
        let mut event = series(
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin"),
            "FREQ=DAILY;COUNT=10",
        );
        let recurrence = event.recurrence.as_mut().unwrap();
        recurrence
            .exceptions
            .push(PimTime::zoned("2026-10-06T09:00:00", "Europe/Berlin"));
        let mut moved = event.fields.clone();
        moved.title = "Moved".into();
        moved.start = PimTime::zoned("2026-10-20T15:00:00", "Europe/Berlin");
        moved.end = PimTime::zoned("2026-10-20T16:00:00", "Europe/Berlin");
        let mut cancelled = event.fields.clone();
        cancelled.status = EventStatus::Cancelled;
        cancelled.start = PimTime::zoned("2026-10-08T09:00:00", "Europe/Berlin");
        cancelled.end = PimTime::zoned("2026-10-08T10:00:00", "Europe/Berlin");
        let mut later = event.fields.clone();
        later.title = "Later".into();
        later.start = PimTime::zoned("2026-10-12T11:00:00", "Europe/Berlin");
        later.end = PimTime::zoned("2026-10-12T11:30:00", "Europe/Berlin");
        event.overrides = vec![
            EventOverride {
                recurrence_id: PimTime::zoned("2026-10-07T09:00:00", "Europe/Berlin"),
                fields: moved,
                this_and_future: false,
            },
            EventOverride {
                recurrence_id: PimTime::utc("2026-10-08T07:00:00"),
                fields: cancelled,
                this_and_future: false,
            },
            EventOverride {
                recurrence_id: PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin"),
                fields: later,
                this_and_future: false,
            },
        ];
        // Recurrence ids are in the start's frame (the parser does that).
        event.overrides[1].recurrence_id = PimTime::zoned("2026-10-08T09:00:00", "Europe/Berlin");
        let future = [PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin")];
        let options = ExpandOptions {
            this_and_future: &future,
            ..ExpandOptions::range(
                utc("2026-10-01T00:00:00Z"),
                utc("2026-11-01T00:00:00Z"),
                Tz::UTC,
            )
        };
        let list = instances(&event, &options);
        let summary: Vec<(String, &str)> = list
            .iter()
            .map(|i| (i.start_utc.clone(), i.title.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("2026-10-05T07:00:00Z".to_owned(), "Standup"),
                ("2026-10-09T07:00:00Z".into(), "Standup"),
                ("2026-10-10T07:00:00Z".into(), "Standup"),
                ("2026-10-11T07:00:00Z".into(), "Standup"),
                ("2026-10-12T09:00:00Z".into(), "Later"),
                ("2026-10-13T09:00:00Z".into(), "Later"),
                ("2026-10-14T09:00:00Z".into(), "Later"),
                ("2026-10-20T13:00:00Z".into(), "Moved"),
            ]
        );
        assert!(list[5].overridden);
        assert_eq!(list[5].end_utc, "2026-10-13T09:30:00Z");
        assert_eq!(
            list[7].recurrence_id,
            Some(PimTime::zoned("2026-10-07T09:00:00", "Europe/Berlin"))
        );
    }

    #[test]
    fn this_and_future_flag_on_the_override_counts_too() {
        let mut event = series(
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"),
            PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin"),
            "FREQ=DAILY;COUNT=4",
        );
        let mut later = event.fields.clone();
        later.title = "Later".into();
        later.start = PimTime::zoned("2026-10-06T11:00:00", "Europe/Berlin");
        later.end = PimTime::zoned("2026-10-06T11:30:00", "Europe/Berlin");
        event.overrides = vec![EventOverride {
            recurrence_id: PimTime::zoned("2026-10-06T09:00:00", "Europe/Berlin"),
            fields: later,
            this_and_future: true,
        }];
        let options = ExpandOptions::range(
            utc("2026-10-01T00:00:00Z"),
            utc("2026-11-01T00:00:00Z"),
            Tz::UTC,
        );
        let list = instances(&event, &options);
        let titles: Vec<(String, &str)> = list
            .iter()
            .map(|i| (i.start_utc.clone(), i.title.as_str()))
            .collect();
        assert_eq!(
            titles,
            [
                ("2026-10-05T07:00:00Z".to_owned(), "Standup"),
                ("2026-10-06T09:00:00Z".into(), "Later"),
                ("2026-10-07T09:00:00Z".into(), "Later"),
                ("2026-10-08T09:00:00Z".into(), "Later"),
            ]
        );
    }

    #[test]
    fn single_events() {
        let mut event = series(
            PimTime::date("2026-10-03"),
            PimTime::date("2026-10-04"),
            "FREQ=DAILY",
        );
        event.recurrence = None;
        let options = ExpandOptions::range(
            utc("2026-10-03T00:00:00Z"),
            utc("2026-10-04T00:00:00Z"),
            Tz::UTC,
        );
        let list = instances(&event, &options);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].recurrence_id, None);
        assert!(!list[0].recurring);
        event.fields.status = EventStatus::Cancelled;
        assert_eq!(instances(&event, &options).len(), 0);
    }

    #[test]
    fn occurrence_start_lists() {
        let mut rule = RecurrenceRule::new(Frequency::Weekly);
        rule.end = RecurrenceEnd::Count { count: 3 };
        let starts = occurrence_starts(
            &PimTime::date("2026-10-01"),
            Some(&Recurrence::rule(rule)),
            utc("2030-01-01T00:00:00Z"),
            Tz::UTC,
            10,
        );
        assert_eq!(
            starts,
            [
                PimTime::date("2026-10-01"),
                PimTime::date("2026-10-08"),
                PimTime::date("2026-10-15")
            ]
        );
        assert_eq!(
            occurrence_starts(
                &PimTime::date("2026-10-01"),
                None,
                utc("2020-01-01T00:00:00Z"),
                Tz::UTC,
                10
            ),
            []
        );
    }

    /// A `SECONDLY` series from 1970 costs next to nothing in a 2026 view
    /// (it ended long ago: MINUTELY / SECONDLY are capped), and an
    /// `HOURLY` series older than `MAX_SCANNED` hours still shows.
    #[test]
    fn old_fine_grained_series_are_cheap_and_correct() {
        let range = ExpandOptions::range(
            utc("2026-10-01T00:00:00Z"),
            utc("2026-10-02T00:00:00Z"),
            Tz::UTC,
        );
        let secondly = series(
            PimTime::utc("1970-01-01T00:00:00"),
            PimTime::utc("1970-01-01T00:00:01"),
            "FREQ=SECONDLY",
        );
        let begun = std::time::Instant::now();
        for _ in 0..200 {
            assert_eq!(instances(&secondly, &range).len(), 0);
        }
        assert!(
            begun.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            begun.elapsed()
        );
        let hourly = series(
            PimTime::utc("1990-01-01T00:30:00"),
            PimTime::utc("1990-01-01T00:45:00"),
            "FREQ=HOURLY;INTERVAL=6",
        );
        assert_eq!(
            starts(&instances(&hourly, &range)),
            [
                "2026-10-01T00:30:00Z",
                "2026-10-01T06:30:00Z",
                "2026-10-01T12:30:00Z",
                "2026-10-01T18:30:00Z"
            ]
        );
        // A long COUNT keeps counting correctly over the skip.
        let counted = series(
            PimTime::utc("2000-01-01T10:00:00"),
            PimTime::utc("2000-01-01T11:00:00"),
            "FREQ=DAILY;COUNT=9771",
        );
        assert_eq!(
            starts(&instances(&counted, &range)),
            ["2026-10-01T10:00:00Z"]
        );
        let ended = series(
            PimTime::utc("2000-01-01T10:00:00"),
            PimTime::utc("2000-01-01T11:00:00"),
            "FREQ=DAILY;COUNT=9770",
        );
        assert_eq!(instances(&ended, &range).len(), 0);
    }

    /// A shared budget bounds a whole request over many costly series and
    /// says when it ran out.
    #[test]
    fn a_shared_budget_bounds_the_request() {
        let costly = series(
            PimTime::utc("2026-01-01T00:00:00"),
            PimTime::utc("2026-01-01T00:00:00"),
            "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30;COUNT=5",
        );
        let budget = ScanBudget::new(1_000);
        let options = ExpandOptions {
            budget: Some(&budget),
            ..ExpandOptions::range(
                utc("2026-10-01T00:00:00Z"),
                utc("2026-11-01T00:00:00Z"),
                Tz::UTC,
            )
        };
        let daily = series(
            PimTime::utc("2026-09-01T09:00:00"),
            PimTime::utc("2026-09-01T10:00:00"),
            "FREQ=DAILY",
        );
        assert_eq!(instances(&daily, &options).len(), 31);
        assert!(!budget.exhausted());
        drop(instances(&costly, &options));
        for _ in 0..2_000 {
            drop(instances(&daily, &options));
        }
        assert!(budget.exhausted());
        assert_eq!(instances(&daily, &options).len(), 0);
    }
}
