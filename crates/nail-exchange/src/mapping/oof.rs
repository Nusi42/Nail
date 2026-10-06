//! Automatic replies (out of office) ↔ the generic vacation settings
//! (ADR 0072): for Exchange accounts the vacation UI edits automatic
//! replies through this mapping.
//!
//! | Vacation | Automatic replies |
//! | --- | --- |
//! | `enabled`, no dates | `always-enabled` |
//! | `enabled` with dates (local days, end inclusive) | `scheduled`, `start` = first day 00:00, `end` = the day after the last 00:00, in the mailbox's zone, as UTC |
//! | `body` | `internal_reply` and `external_reply` (audience kept, `all` by default) |
//! | `subject`, `days`, `addresses`, `from` | not kept (notes `exchange-oof-note-*`) |
//!
//! A start without an end becomes a schedule of [`OPEN_END_DAYS`] days;
//! an end without a start starts now.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use nail_api::exchange::oof::ExternalAudience;
use nail_api::exchange::{AutomaticRepliesSettings, AutomaticRepliesStatus};
use nail_api::localized::LocalizedMessage;
use nail_api::sieve::VacationSettings;

use crate::texts;

/// Length of a schedule that has a start but no end (days).
pub const OPEN_END_DAYS: i64 = 365;

/// A mapped value and what did not carry over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped<T> {
    /// The value.
    pub value: T,
    /// Notes for the user (`exchange-oof-note-*`).
    pub notes: Vec<LocalizedMessage>,
}

fn utc_text(instant: DateTime<Utc>) -> String {
    instant.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_utc(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Local midnight of `date` in `zone` as UTC (the first valid instant of
/// the day when midnight does not exist).
fn midnight(date: NaiveDate, zone: Tz) -> DateTime<Utc> {
    let local = date.and_time(NaiveTime::MIN);
    match zone.from_local_datetime(&local) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.with_timezone(&Utc)
        }
        chrono::LocalResult::None => zone
            .from_local_datetime(&(local + Duration::hours(1)))
            .earliest()
            .map_or_else(|| Utc.from_utc_datetime(&local), |t| t.with_timezone(&Utc)),
    }
}

fn parse_date(text: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text?.trim(), "%Y-%m-%d").ok()
}

/// Vacation settings as automatic replies. `zone` is the mailbox's zone
/// (dates are its days); `now` starts a schedule without a start;
/// `current` keeps the external audience the server had.
pub fn automatic_replies_of(
    vacation: &VacationSettings,
    zone: Tz,
    now: DateTime<Utc>,
    current: Option<&AutomaticRepliesSettings>,
) -> Mapped<AutomaticRepliesSettings> {
    let mut notes = Vec::new();
    if !vacation.subject.trim().is_empty() {
        notes.push(LocalizedMessage::new(texts::OOF_NOTE_SUBJECT));
    }
    if vacation.days != VacationSettings::DEFAULT_DAYS {
        notes.push(LocalizedMessage::new(texts::OOF_NOTE_DAYS));
    }
    if !vacation.addresses.is_empty() || vacation.from.is_some() {
        notes.push(LocalizedMessage::new(texts::OOF_NOTE_ADDRESSES));
    }
    let audience = current.map_or(ExternalAudience::All, |c| c.external_audience);
    let body = vacation.body.trim().to_owned();
    let start = parse_date(vacation.start_date.as_deref()).map(|d| midnight(d, zone));
    let end = parse_date(vacation.end_date.as_deref())
        .and_then(|d| d.succ_opt())
        .map(|d| midnight(d, zone));
    let (status, start, end) = if vacation.enabled {
        match (start, end) {
            (None, None) => (AutomaticRepliesStatus::AlwaysEnabled, None, None),
            (Some(start), Some(end)) => (AutomaticRepliesStatus::Scheduled, Some(start), Some(end)),
            (Some(start), None) => {
                notes.push(LocalizedMessage::new(texts::OOF_NOTE_OPEN_END));
                (
                    AutomaticRepliesStatus::Scheduled,
                    Some(start),
                    Some(start + Duration::days(OPEN_END_DAYS)),
                )
            }
            (None, Some(end)) => {
                let now = now.with_timezone(&Utc);
                (AutomaticRepliesStatus::Scheduled, Some(now), Some(end))
            }
        }
    } else {
        (AutomaticRepliesStatus::Disabled, None, None)
    };
    Mapped {
        value: AutomaticRepliesSettings {
            status,
            start: start.map(utc_text),
            end: end.map(utc_text),
            internal_reply: body.clone(),
            external_reply: body,
            external_audience: audience,
        },
        notes,
    }
}

/// Automatic replies as vacation settings for the generic vacation UI
/// (`zone` = the mailbox's zone).
pub fn vacation_of(replies: &AutomaticRepliesSettings, zone: Tz) -> Mapped<VacationSettings> {
    let mut notes = Vec::new();
    let mut vacation = VacationSettings {
        enabled: replies.status != AutomaticRepliesStatus::Disabled,
        body: replies.internal_reply.clone(),
        ..VacationSettings::default()
    };
    if replies.external_audience != ExternalAudience::None
        && replies.external_reply.trim() != replies.internal_reply.trim()
    {
        notes.push(LocalizedMessage::new(texts::OOF_NOTE_EXTERNAL));
    }
    if replies.status == AutomaticRepliesStatus::Scheduled {
        let start = replies.start.as_deref().and_then(parse_utc);
        let end = replies.end.as_deref().and_then(parse_utc);
        let local = |t: DateTime<Utc>| t.with_timezone(&zone).naive_local();
        let mut rounded = false;
        if let Some(start) = start {
            let l = local(start);
            rounded |= l.time() != NaiveTime::MIN;
            vacation.start_date = Some(l.date().format("%Y-%m-%d").to_string());
        }
        if let Some(end) = end {
            let l = local(end);
            rounded |= l.time() != NaiveTime::MIN;
            // The end is exclusive; the vacation's last day inclusive.
            let last = local(end - Duration::seconds(1)).date();
            vacation.end_date = Some(last.format("%Y-%m-%d").to_string());
        }
        if rounded {
            notes.push(LocalizedMessage::new(texts::OOF_NOTE_ROUNDED));
        }
    }
    Mapped {
        value: vacation,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn berlin() -> Tz {
        "Europe/Berlin".parse().unwrap()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap()
    }

    fn ids(notes: &[LocalizedMessage]) -> Vec<&str> {
        notes.iter().map(|n| n.id.as_str()).collect()
    }

    #[test]
    fn scheduled_vacation_round_trips() {
        let vacation = VacationSettings {
            enabled: true,
            body: "Back on Monday.".into(),
            start_date: Some("2026-10-05".into()),
            end_date: Some("2026-10-09".into()),
            ..VacationSettings::default()
        };
        let mapped = automatic_replies_of(&vacation, berlin(), now(), None);
        assert!(mapped.notes.is_empty(), "{:?}", mapped.notes);
        let replies = mapped.value;
        assert_eq!(replies.status, AutomaticRepliesStatus::Scheduled);
        assert_eq!(replies.start.as_deref(), Some("2026-10-04T22:00:00Z"));
        assert_eq!(replies.end.as_deref(), Some("2026-10-09T22:00:00Z"));
        assert_eq!(replies.external_reply, "Back on Monday.");
        assert_eq!(replies.external_audience, ExternalAudience::All);
        assert_eq!(replies.issues().len(), 0);
        let back = vacation_of(&replies, berlin());
        assert!(back.notes.is_empty(), "{:?}", back.notes);
        assert_eq!(back.value, vacation);
    }

    #[test]
    fn dates_across_a_dst_change() {
        // Berlin leaves summer time on 2026-10-25.
        let vacation = VacationSettings {
            enabled: true,
            body: "x".into(),
            start_date: Some("2026-10-24".into()),
            end_date: Some("2026-10-26".into()),
            ..VacationSettings::default()
        };
        let replies = automatic_replies_of(&vacation, berlin(), now(), None).value;
        assert_eq!(replies.start.as_deref(), Some("2026-10-23T22:00:00Z"));
        assert_eq!(replies.end.as_deref(), Some("2026-10-26T23:00:00Z"));
        assert_eq!(
            vacation_of(&replies, berlin()).value.end_date,
            vacation.end_date
        );
    }

    #[test]
    fn always_open_end_and_disabled() {
        let on = VacationSettings {
            enabled: true,
            body: "Away".into(),
            subject: "Out".into(),
            days: 1,
            addresses: vec!["ada@home.example".into()],
            ..VacationSettings::default()
        };
        let mapped = automatic_replies_of(&on, berlin(), now(), None);
        assert_eq!(mapped.value.status, AutomaticRepliesStatus::AlwaysEnabled);
        assert_eq!(
            ids(&mapped.notes),
            [
                texts::OOF_NOTE_SUBJECT,
                texts::OOF_NOTE_DAYS,
                texts::OOF_NOTE_ADDRESSES
            ]
        );
        let open = VacationSettings {
            start_date: Some("2026-10-05".into()),
            subject: String::new(),
            days: VacationSettings::DEFAULT_DAYS,
            addresses: Vec::new(),
            ..on.clone()
        };
        let mapped = automatic_replies_of(&open, berlin(), now(), None);
        assert_eq!(ids(&mapped.notes), [texts::OOF_NOTE_OPEN_END]);
        assert_eq!(mapped.value.end.as_deref(), Some("2027-10-04T22:00:00Z"));
        let until = VacationSettings {
            start_date: None,
            end_date: Some("2026-10-02".into()),
            ..open.clone()
        };
        let mapped = automatic_replies_of(&until, berlin(), now(), None);
        assert_eq!(mapped.value.start.as_deref(), Some("2026-10-01T12:00:00Z"));
        let off = VacationSettings {
            enabled: false,
            ..open
        };
        let current = AutomaticRepliesSettings {
            external_audience: ExternalAudience::ContactsOnly,
            ..AutomaticRepliesSettings::default()
        };
        let mapped = automatic_replies_of(&off, berlin(), now(), Some(&current));
        assert_eq!(mapped.value.status, AutomaticRepliesStatus::Disabled);
        assert_eq!(mapped.value.start, None);
        assert_eq!(
            mapped.value.external_audience,
            ExternalAudience::ContactsOnly
        );
    }

    #[test]
    fn server_settings_as_vacation() {
        let replies = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::Scheduled,
            start: Some("2026-10-05T07:30:00Z".into()),
            end: Some("2026-10-09T16:00:00Z".into()),
            internal_reply: "Inside".into(),
            external_reply: "Outside".into(),
            external_audience: ExternalAudience::All,
        };
        let mapped = vacation_of(&replies, berlin());
        assert_eq!(
            ids(&mapped.notes),
            [texts::OOF_NOTE_EXTERNAL, texts::OOF_NOTE_ROUNDED]
        );
        assert_eq!(mapped.value.start_date.as_deref(), Some("2026-10-05"));
        assert_eq!(mapped.value.end_date.as_deref(), Some("2026-10-09"));
        assert_eq!(mapped.value.body, "Inside");
        assert!(mapped.value.enabled);
        let inside_only = AutomaticRepliesSettings {
            status: AutomaticRepliesStatus::AlwaysEnabled,
            external_audience: ExternalAudience::None,
            ..replies
        };
        let mapped = vacation_of(&inside_only, berlin());
        assert!(mapped.notes.is_empty(), "{:?}", mapped.notes);
        assert_eq!(mapped.value.start_date, None);
        assert!(
            !vacation_of(&AutomaticRepliesSettings::default(), berlin())
                .value
                .enabled
        );
    }
}
