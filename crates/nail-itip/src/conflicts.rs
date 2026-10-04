//! Conflicts: busy time in the user's calendars that overlaps an invited
//! time (the card's `conflicts`, attendee side), and busy periods that
//! overlap a proposed time (free/busy of other people).
//!
//! The invited times are the occurrences of the invitation in a window
//! (recurring invitations: the first [`MAX_INVITED_OCCURRENCES`] within
//! the window). Instances come from the store's instance cache (the core
//! asks it for the window); free (`TRANSP:TRANSPARENT`) and cancelled
//! instances never conflict, and the invitation's own event is skipped.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use nail_api::ids::EventId;
use nail_api::pim::calendar::{CalendarEvent, EventInstance, EventStatus, Transparency};
use nail_api::scheduling::{BusyKind, BusyPeriod, InvitationConflict};
use nail_ical::expand::{self, ExpandOptions};
use nail_ical::time;

use crate::object::ItipObject;

/// Most occurrences of a recurring invitation checked.
pub const MAX_INVITED_OCCURRENCES: usize = 50;

/// Most conflicts reported.
pub const MAX_CONFLICTS: usize = 50;

/// Default window for recurring invitations, in days from the first
/// occurrence.
pub const DEFAULT_WINDOW_DAYS: i64 = 92;

/// One invited stretch of time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitedSpan {
    /// Start.
    pub start: DateTime<Utc>,
    /// End (exclusive; equal to `start` for zero-length events).
    pub end: DateTime<Utc>,
}

/// The invited times of an iTIP object's event in `[from, to)`: its
/// occurrences (overrides applied), at most [`MAX_INVITED_OCCURRENCES`].
/// Floating and all-day times are placed in `viewer`.
pub fn invited_spans(
    object: &ItipObject,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    viewer: Tz,
) -> Vec<InvitedSpan> {
    let Some(parsed) = object.event.clone() else {
        // Lone occurrences (an occurrence REQUEST / ADD / COUNTER).
        return object
            .components
            .iter()
            .filter_map(|c| c.fields.as_ref())
            .filter(|f| f.status != EventStatus::Cancelled)
            .filter_map(|f| span(&f.start, &f.end, viewer))
            .filter(|s| s.start < to && s.end >= from)
            .take(MAX_INVITED_OCCURRENCES)
            .collect();
    };
    let this_and_future = parsed.this_and_future.clone();
    let event = parsed.into_event(
        "itip".into(),
        "itip".into(),
        true,
        nail_api::pim::PimItemSync::default(),
    );
    event_spans(&event, from, to, viewer, &this_and_future)
}

/// The occurrence spans of a stored event in `[from, to)`.
pub fn event_spans(
    event: &CalendarEvent,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    viewer: Tz,
    this_and_future: &[nail_api::pim::PimTime],
) -> Vec<InvitedSpan> {
    let mut options = ExpandOptions::range(from, to, viewer);
    options.limit = MAX_INVITED_OCCURRENCES;
    options.this_and_future = this_and_future;
    expand::instances(event, &options)
        .iter()
        .filter_map(|i| {
            Some(InvitedSpan {
                start: time::parse_instant(&i.start_utc)?,
                end: time::parse_instant(&i.end_utc)?,
            })
        })
        .collect()
}

fn span(
    start: &nail_api::pim::PimTime,
    end: &nail_api::pim::PimTime,
    viewer: Tz,
) -> Option<InvitedSpan> {
    let s = time::instant(start, viewer)?;
    let e = time::instant(end, viewer)?.max(s);
    Some(InvitedSpan { start: s, end: e })
}

/// The window to check for an invitation: [`DEFAULT_WINDOW_DAYS`] from
/// its first invited time, but starting at most a day before `now` (a
/// series that began long ago is checked from now on).
pub fn window(
    object: &ItipObject,
    now: DateTime<Utc>,
    viewer: Tz,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let first = object
        .main()
        .and_then(|c| c.fields.as_ref())
        .and_then(|f| time::instant(&f.start, viewer));
    let from = first.map_or(now, |f| f.max(now - Duration::days(1)));
    (from, from + Duration::days(DEFAULT_WINDOW_DAYS))
}

fn overlaps(
    a_start: DateTime<Utc>,
    a_end: DateTime<Utc>,
    b_start: DateTime<Utc>,
    b_end: DateTime<Utc>,
) -> bool {
    if a_start == a_end {
        return a_start >= b_start && a_start < b_end;
    }
    if b_start == b_end {
        return b_start >= a_start && b_start < a_end;
    }
    a_start < b_end && b_start < a_end
}

/// Instances of the user's calendars that overlap invited spans.
/// `ignore` is the invitation's own event (already in a calendar).
pub fn find_conflicts(
    invited: &[InvitedSpan],
    instances: &[EventInstance],
    ignore: Option<&EventId>,
) -> Vec<InvitationConflict> {
    let mut out: Vec<(DateTime<Utc>, InvitationConflict)> = Vec::new();
    for i in instances {
        if Some(&i.event_id) == ignore
            || i.status == EventStatus::Cancelled
            || i.transparency == Transparency::Free
        {
            continue;
        }
        let (Some(start), Some(end)) = (
            time::parse_instant(&i.start_utc),
            time::parse_instant(&i.end_utc),
        ) else {
            continue;
        };
        if !invited.iter().any(|s| overlaps(s.start, s.end, start, end)) {
            continue;
        }
        if out
            .iter()
            .any(|(_, c)| c.event_id == i.event_id && c.recurrence_id == i.recurrence_id)
        {
            continue;
        }
        out.push((
            start,
            InvitationConflict {
                event_id: i.event_id.clone(),
                calendar_id: i.calendar_id.clone(),
                recurrence_id: i.recurrence_id.clone(),
                title: i.title.clone(),
                start_utc: i.start_utc.clone(),
                end_utc: i.end_utc.clone(),
                all_day: i.all_day,
                tentative: i.status == EventStatus::Tentative,
            },
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.title.cmp(&b.1.title)));
    out.into_iter()
        .take(MAX_CONFLICTS)
        .map(|(_, c)| c)
        .collect()
}

/// Busy periods (free/busy of attendees) that overlap invited spans,
/// e.g. to warn about a proposed time.
pub fn busy_conflicts(invited: &[InvitedSpan], periods: &[BusyPeriod]) -> Vec<BusyPeriod> {
    let mut out: Vec<BusyPeriod> = periods
        .iter()
        .filter(|p| {
            let (Some(start), Some(end)) =
                (time::parse_instant(&p.start), time::parse_instant(&p.end))
            else {
                return false;
            };
            invited.iter().any(|s| overlaps(s.start, s.end, start, end))
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| (&a.start, a.kind).cmp(&(&b.start, b.kind)));
    out
}

/// Whether any busy period (other than tentative) overlaps the spans.
pub fn is_busy(invited: &[InvitedSpan], periods: &[BusyPeriod]) -> bool {
    busy_conflicts(invited, periods)
        .iter()
        .any(|p| p.kind != BusyKind::Tentative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::parse_itip;
    use nail_api::pim::PimTime;

    fn at(text: &str) -> DateTime<Utc> {
        time::parse_instant(text).unwrap()
    }

    fn instance(id: &str, start: &str, end: &str) -> EventInstance {
        EventInstance {
            event_id: id.into(),
            calendar_id: "cal".into(),
            recurrence_id: None,
            start: PimTime::utc(start.trim_end_matches('Z')),
            end: PimTime::utc(end.trim_end_matches('Z')),
            start_utc: start.into(),
            end_utc: end.into(),
            all_day: false,
            title: id.into(),
            location: None,
            status: EventStatus::Confirmed,
            transparency: Transparency::Busy,
            color: None,
            recurring: false,
            overridden: false,
            has_attendees: false,
            has_alarms: false,
            read_only: false,
        }
    }

    const WEEKLY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//T//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:w\r\nDTSTAMP:20261001T000000Z\r\nDTSTART;TZID=Europe/Berlin:20261005T090000\r\n\
DTEND;TZID=Europe/Berlin:20261005T100000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nSUMMARY:Weekly\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:w\r\nDTSTAMP:20261001T000000Z\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261012T090000\r\n\
DTSTART;TZID=Europe/Berlin:20261012T140000\r\nDTEND;TZID=Europe/Berlin:20261012T150000\r\nSUMMARY:Weekly\r\n\
ORGANIZER:mailto:ada@example.org\r\nATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn recurring_invitations_expand_with_overrides() {
        let object = parse_itip(WEEKLY, None);
        let (from, to) = window(&object, at("2026-10-01T00:00:00Z"), Tz::UTC);
        assert_eq!(from, at("2026-10-05T07:00:00Z"));
        let spans = invited_spans(&object, from, to, Tz::UTC);
        assert_eq!(
            spans.iter().map(|s| s.start).collect::<Vec<_>>(),
            [
                at("2026-10-05T07:00:00Z"),
                at("2026-10-12T12:00:00Z"),
                at("2026-10-19T07:00:00Z")
            ]
        );
        let busy = [
            instance("standup", "2026-10-05T07:30:00Z", "2026-10-05T07:45:00Z"),
            // The moved occurrence is at 12:00 UTC, not 07:00.
            instance(
                "free-at-seven",
                "2026-10-12T07:00:00Z",
                "2026-10-12T08:00:00Z",
            ),
            instance("lunch", "2026-10-12T11:30:00Z", "2026-10-12T12:30:00Z"),
            // Touching is no conflict.
            instance("after", "2026-10-19T08:00:00Z", "2026-10-19T09:00:00Z"),
            {
                let mut i = instance(
                    "transparent",
                    "2026-10-19T07:00:00Z",
                    "2026-10-19T08:00:00Z",
                );
                i.transparency = Transparency::Free;
                i
            },
            {
                let mut i = instance("cancelled", "2026-10-19T07:00:00Z", "2026-10-19T08:00:00Z");
                i.status = EventStatus::Cancelled;
                i
            },
            {
                let mut i = instance("maybe", "2026-10-19T07:15:00Z", "2026-10-19T07:20:00Z");
                i.status = EventStatus::Tentative;
                i
            },
            instance("self", "2026-10-05T07:00:00Z", "2026-10-05T08:00:00Z"),
        ];
        let conflicts = find_conflicts(&spans, &busy, Some(&"self".into()));
        let titles: Vec<&str> = conflicts.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["standup", "lunch", "maybe"]);
        assert!(conflicts[2].tentative && !conflicts[0].tentative);
    }

    #[test]
    fn lone_occurrences_and_zero_length_events() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//T//EN\r\nMETHOD:COUNTER\r\n\
BEGIN:VEVENT\r\nUID:c\r\nDTSTAMP:20261001T000000Z\r\nRECURRENCE-ID:20261005T070000Z\r\n\
DTSTART:20261005T150000Z\r\nDTEND:20261005T160000Z\r\nORGANIZER:mailto:ada@example.org\r\n\
ATTENDEE:mailto:bob@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let object = parse_itip(text, None);
        let spans = invited_spans(
            &object,
            at("2026-10-01T00:00:00Z"),
            at("2026-11-01T00:00:00Z"),
            Tz::UTC,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, at("2026-10-05T15:00:00Z"));
        let point = [instance(
            "deadline",
            "2026-10-05T15:30:00Z",
            "2026-10-05T15:30:00Z",
        )];
        assert_eq!(find_conflicts(&spans, &point, None).len(), 1);
        let outside = [instance(
            "later",
            "2026-10-05T16:00:00Z",
            "2026-10-05T16:00:00Z",
        )];
        assert!(find_conflicts(&spans, &outside, None).is_empty());
    }

    #[test]
    fn busy_periods() {
        let spans = [InvitedSpan {
            start: at("2026-10-05T09:00:00Z"),
            end: at("2026-10-05T10:00:00Z"),
        }];
        let p = |s: &str, e: &str, kind| BusyPeriod {
            start: format!("2026-10-05T{s}:00Z"),
            end: format!("2026-10-05T{e}:00Z"),
            kind,
        };
        let periods = [
            p("09:30", "11:00", BusyKind::Tentative),
            p("08:00", "09:00", BusyKind::Busy),
            p("09:59", "10:30", BusyKind::Unavailable),
            BusyPeriod {
                start: "bad".into(),
                end: "bad".into(),
                kind: BusyKind::Busy,
            },
        ];
        let hits = busy_conflicts(&spans, &periods);
        assert_eq!(
            hits,
            [
                p("09:30", "11:00", BusyKind::Tentative),
                p("09:59", "10:30", BusyKind::Unavailable)
            ]
        );
        assert!(is_busy(&spans, &periods));
        assert!(!is_busy(&spans, &periods[..2]));
    }
}
