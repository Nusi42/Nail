//! Ordering of iTIP messages (RFC 5546 §2.1.4, §2.1.5) and the
//! organizer's `SEQUENCE` rules (RFC 5545 §3.8.7.4).
//!
//! A message is newer than what the calendar has when its `SEQUENCE` is
//! higher, or the same with a later `DTSTAMP`. A message without
//! `DTSTAMP` is never newer at the same `SEQUENCE` (the store's
//! `itip_is_newer` uses the same rule, so the card and the stored
//! scheduling state agree). Stale messages are ignored.
//!
//! The organizer bumps `SEQUENCE` on *significant* changes: start, end
//! (or duration), recurrence, location, status `CANCELLED`, overridden
//! occurrences moving or appearing / disappearing. Attendee answers then
//! go back to `NEEDS-ACTION` (with `RSVP=TRUE`) so everyone confirms the
//! new time.

use std::cmp::Ordering;

use nail_api::pim::PimTime;
use nail_api::pim::calendar::ParticipationStatus;
use nail_api::pim::calendar::{CalendarEvent, EventFields, EventStatus};
use nail_ical::time;

/// The version of an event in scheduling terms: `SEQUENCE`, then
/// `DTSTAMP` (RFC 3339 UTC).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version {
    /// `SEQUENCE`.
    pub sequence: u32,
    /// `DTSTAMP` (RFC 3339 UTC, `YYYY-MM-DDTHH:MM:SSZ`).
    pub dtstamp: Option<String>,
}

impl Version {
    /// A version.
    pub fn new(sequence: u32, dtstamp: Option<String>) -> Self {
        Self { sequence, dtstamp }
    }

    /// Whether this version is newer than `other` (higher `SEQUENCE`, or
    /// the same with a later `DTSTAMP`; a missing `DTSTAMP` is never
    /// newer).
    pub fn is_newer_than(&self, other: &Version) -> bool {
        self.cmp(other) == Ordering::Greater
    }

    /// Whether this version is older than `other`.
    pub fn is_older_than(&self, other: &Version) -> bool {
        other.is_newer_than(self)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sequence
            .cmp(&other.sequence)
            .then_with(|| match (&self.dtstamp, &other.dtstamp) {
                (Some(a), Some(b)) => stamp_key(a).cmp(&stamp_key(b)),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => Ordering::Equal,
            })
    }
}

/// Compares stamps as instants where possible (RFC 3339 with offsets or
/// fractions), else as text.
fn stamp_key(stamp: &str) -> (i64, String) {
    match time::parse_instant(stamp) {
        Some(instant) => (instant.timestamp(), String::new()),
        None => (i64::MIN, stamp.to_owned()),
    }
}

/// Whether two field sets differ in a way that needs everyone's answer
/// again: start, end, location or cancellation.
pub fn fields_change_significantly(old: &EventFields, new: &EventFields) -> bool {
    !same_time(&old.start, &new.start)
        || !same_time(&old.end, &new.end)
        || trimmed(&old.location) != trimmed(&new.location)
        || (old.status != new.status && new.status == EventStatus::Cancelled)
}

fn trimmed(text: &Option<String>) -> Option<&str> {
    text.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

/// Equal times, also when written in different zones (same instant).
fn same_time(a: &PimTime, b: &PimTime) -> bool {
    if a == b {
        return true;
    }
    if a.is_date() != b.is_date() {
        return false;
    }
    let utc = chrono_tz::Tz::UTC;
    match (a, b) {
        (PimTime::Zoned { .. }, PimTime::Zoned { .. }) => {
            time::instant(a, utc).is_some() && time::instant(a, utc) == time::instant(b, utc)
        }
        _ => false,
    }
}

/// Whether an organizer's edit of an event is significant (RFC 5545
/// §3.8.7.4): the master's time / location / cancellation, the
/// recurrence, or overridden occurrences that moved, appeared or went.
pub fn is_significant_change(old: &CalendarEvent, new: &CalendarEvent) -> bool {
    if fields_change_significantly(&old.fields, &new.fields) || old.recurrence != new.recurrence {
        return true;
    }
    if old.overrides.len() != new.overrides.len() {
        return true;
    }
    new.overrides.iter().any(|o| {
        old.overrides
            .iter()
            .find(|p| p.recurrence_id == o.recurrence_id)
            .is_none_or(|p| fields_change_significantly(&p.fields, &o.fields))
    })
}

/// The organizer's copy prepared for sending an update: `SEQUENCE` goes
/// up by one on a significant change (relative to `sent`, the version the
/// attendees have) and every attendee's answer is reset to
/// `NEEDS-ACTION` with `RSVP=TRUE` (the organizer's own entry, if listed,
/// keeps its answer). Returns whether it was significant.
pub fn prepare_update(
    sent: &CalendarEvent,
    updated: &mut CalendarEvent,
    organizer_address: &str,
) -> bool {
    let significant = is_significant_change(sent, updated);
    if significant {
        updated.sequence = sent.sequence.max(updated.sequence) + 1;
        let reset = |fields: &mut EventFields| {
            for attendee in &mut fields.attendees {
                if crate::address::same(&attendee.email, organizer_address) {
                    continue;
                }
                if attendee.status != ParticipationStatus::Delegated {
                    attendee.status = ParticipationStatus::NeedsAction;
                    attendee.rsvp = true;
                }
            }
        };
        reset(&mut updated.fields);
        for o in &mut updated.overrides {
            reset(&mut o.fields);
        }
    } else {
        updated.sequence = sent.sequence.max(updated.sequence);
    }
    significant
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::calendar::{Attendee, EventOverride, Frequency, Recurrence, RecurrenceRule};

    fn v(sequence: u32, stamp: Option<&str>) -> Version {
        Version::new(sequence, stamp.map(str::to_owned))
    }

    #[test]
    fn versions_order_by_sequence_then_dtstamp() {
        assert!(v(2, None).is_newer_than(&v(1, Some("2027-01-01T00:00:00Z"))));
        assert!(
            v(1, Some("2026-10-01T09:00:00Z")).is_newer_than(&v(1, Some("2026-10-01T08:00:00Z")))
        );
        assert!(
            !v(1, Some("2026-10-01T08:00:00Z")).is_newer_than(&v(1, Some("2026-10-01T08:00:00Z")))
        );
        assert!(v(1, Some("2026-10-01T08:00:00Z")).is_newer_than(&v(1, None)));
        assert!(!v(1, None).is_newer_than(&v(1, Some("2026-10-01T08:00:00Z"))));
        assert!(!v(1, None).is_newer_than(&v(1, None)));
        assert!(v(0, None).is_older_than(&v(1, None)));
        // Offsets compare as instants.
        assert!(
            v(1, Some("2026-10-01T10:30:00+02:00"))
                .is_newer_than(&v(1, Some("2026-10-01T08:00:00Z")))
        );
        assert_eq!(
            v(1, Some("2026-10-01T10:00:00+02:00")).cmp(&v(1, Some("2026-10-01T08:00:00Z"))),
            Ordering::Equal
        );
    }

    fn event(start: &str, end: &str) -> CalendarEvent {
        let fields = EventFields::new(
            "Planning",
            PimTime::zoned(start, "Europe/Berlin"),
            PimTime::zoned(end, "Europe/Berlin"),
        );
        CalendarEvent {
            id: "ev".into(),
            calendar_id: "cal".into(),
            uid: "u".into(),
            fields,
            recurrence: None,
            overrides: Vec::new(),
            sequence: 3,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: Default::default(),
        }
    }

    #[test]
    fn significant_changes() {
        let old = event("2026-10-05T09:00:00", "2026-10-05T10:00:00");
        let mut same = old.clone();
        same.fields.description = Some("Agenda".into());
        same.fields.title = "Planning!".into();
        assert!(!is_significant_change(&old, &same));
        let mut moved = old.clone();
        moved.fields.start = PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin");
        assert!(is_significant_change(&old, &moved));
        // The same instant in another zone is no change.
        let mut utc = old.clone();
        utc.fields.start = PimTime::utc("2026-10-05T07:00:00");
        assert!(!is_significant_change(&old, &utc));
        let mut place = old.clone();
        place.fields.location = Some("Room 2".into());
        assert!(is_significant_change(&old, &place));
        let mut blank = old.clone();
        blank.fields.location = Some("  ".into());
        assert!(!is_significant_change(&old, &blank));
        let mut cancelled = old.clone();
        cancelled.fields.status = EventStatus::Cancelled;
        assert!(is_significant_change(&old, &cancelled));
        let mut tentative = old.clone();
        tentative.fields.status = EventStatus::Tentative;
        assert!(!is_significant_change(&old, &tentative));
        let mut recurring = old.clone();
        recurring.recurrence = Some(Recurrence::rule(RecurrenceRule::new(Frequency::Weekly)));
        assert!(is_significant_change(&old, &recurring));
        let mut with_override = recurring.clone();
        with_override.overrides.push(EventOverride {
            recurrence_id: PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin"),
            fields: recurring.fields.clone(),
            this_and_future: false,
        });
        assert!(is_significant_change(&recurring, &with_override));
        let mut retitled = with_override.clone();
        retitled.overrides[0].fields.title = "Other".into();
        assert!(!is_significant_change(&with_override, &retitled));
        let mut moved_override = with_override.clone();
        moved_override.overrides[0].fields.start =
            PimTime::zoned("2026-10-12T11:00:00", "Europe/Berlin");
        assert!(is_significant_change(&with_override, &moved_override));
    }

    #[test]
    fn updates_bump_sequence_and_reset_answers() {
        let mut sent = event("2026-10-05T09:00:00", "2026-10-05T10:00:00");
        let mut bob = Attendee::new("bob@example.org");
        bob.status = ParticipationStatus::Accepted;
        let mut ada = Attendee::new("ada@example.org");
        ada.status = ParticipationStatus::Accepted;
        let mut dan = Attendee::new("dan@example.org");
        dan.status = ParticipationStatus::Delegated;
        sent.fields.attendees = vec![ada, bob, dan];
        let mut minor = sent.clone();
        minor.fields.description = Some("x".into());
        assert!(!prepare_update(&sent, &mut minor, "ada@example.org"));
        assert_eq!(minor.sequence, 3);
        assert_eq!(
            minor.fields.attendees[1].status,
            ParticipationStatus::Accepted
        );

        let mut moved = sent.clone();
        moved.fields.end = PimTime::zoned("2026-10-05T11:00:00", "Europe/Berlin");
        assert!(prepare_update(&sent, &mut moved, "mailto:ADA@example.org"));
        assert_eq!(moved.sequence, 4);
        assert_eq!(
            moved.fields.attendees[0].status,
            ParticipationStatus::Accepted
        );
        assert_eq!(
            moved.fields.attendees[1].status,
            ParticipationStatus::NeedsAction
        );
        assert!(moved.fields.attendees[1].rsvp);
        assert_eq!(
            moved.fields.attendees[2].status,
            ParticipationStatus::Delegated
        );
    }
}
