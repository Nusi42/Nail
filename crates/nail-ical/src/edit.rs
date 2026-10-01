//! Edits of recurring events and tasks by [`EditScope`] (ADR 0058):
//!
//! * `this` — [`override_occurrence`] writes an override, and
//!   [`delete_occurrence`] an `EXDATE`;
//! * `this-and-following` — [`split_series`] ends the series before the
//!   occurrence (`UNTIL`) and returns a new series that starts with it
//!   (`COUNT`s, `RDATE`s, `EXDATE`s and overrides are shared out);
//!   [`end_before`] alone deletes "this and following";
//! * `all` — [`move_series`] shifts the series and its recurrence ids,
//!   `RDATE`s and `EXDATE`s with the start.
//!
//! Recurring tasks move on when completed ([`advance_task`]).

use chrono::{Duration, NaiveDateTime};
use chrono_tz::Tz;
use nail_api::ids::EventId;
use nail_api::pim::calendar::EditScope;
use nail_api::pim::source::PimItemSync;
use nail_api::pim::{
    CalendarEvent, EventFields, EventOverride, PimIssue, PimTime, Recurrence, RecurrenceEnd,
    TaskFields,
};

use crate::recur::{Occurrences, RuleIter, UntilBound};
use crate::time;

/// Why an edit is not possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// The event does not recur.
    #[error("the event does not recur")]
    NotRecurring,
    /// The recurrence id is not an occurrence of the series.
    #[error("not an occurrence of the series")]
    NotAnOccurrence,
    /// "This and following" on the first occurrence (that is "all").
    #[error("the first occurrence: edit the whole series")]
    FirstOccurrence,
    /// A time could not be read.
    #[error("malformed time")]
    MalformedTime,
}

impl EditError {
    /// The contract's validation issue for it.
    pub fn issue(self) -> PimIssue {
        match self {
            Self::MalformedTime => PimIssue::InvalidTime,
            _ => PimIssue::InvalidScope,
        }
    }
}

fn frame(t: &PimTime, start: &PimTime) -> Result<NaiveDateTime, EditError> {
    time::to_frame(t, start).ok_or(EditError::MalformedTime)
}

fn recurrence(event: &CalendarEvent) -> Result<&Recurrence, EditError> {
    event
        .recurrence
        .as_ref()
        .filter(|r| !r.is_empty())
        .ok_or(EditError::NotRecurring)
}

/// Whether `recurrence_id` is an occurrence of the series (generated and
/// not excluded, or an existing override).
pub fn is_occurrence(event: &CalendarEvent, recurrence_id: &PimTime) -> bool {
    let start = &event.fields.start;
    let Ok(target) = frame(recurrence_id, start) else {
        return false;
    };
    if event
        .overrides
        .iter()
        .any(|o| time::to_frame(&o.recurrence_id, start) == Some(target))
    {
        return true;
    }
    Occurrences::new(start, event.recurrence.as_ref()).is_some_and(|mut all| {
        all.by_ref()
            .take_while(|o| *o <= target)
            .any(|o| o == target)
    })
}

/// Checks a scope against an event (`this` / `this-and-following` need a
/// recurring event and an occurrence).
pub fn check_scope(
    event: &CalendarEvent,
    scope: EditScope,
    recurrence_id: Option<&PimTime>,
) -> Result<(), EditError> {
    if !scope.needs_occurrence() {
        return Ok(());
    }
    recurrence(event)?;
    match recurrence_id {
        Some(rid) if is_occurrence(event, rid) => Ok(()),
        _ => Err(EditError::NotAnOccurrence),
    }
}

/// `this`: sets the fields of one occurrence (replacing an override).
pub fn override_occurrence(
    event: &mut CalendarEvent,
    recurrence_id: &PimTime,
    fields: EventFields,
) -> Result<(), EditError> {
    check_scope(event, EditScope::This, Some(recurrence_id))?;
    let start = event.fields.start.clone();
    let target = frame(recurrence_id, &start)?;
    let recurrence_id = time::like(target, &start);
    event
        .overrides
        .retain(|o| time::to_frame(&o.recurrence_id, &start) != Some(target));
    event.overrides.push(EventOverride {
        recurrence_id,
        fields,
        this_and_future: false,
    });
    event
        .overrides
        .sort_by_key(|o| time::to_frame(&o.recurrence_id, &start));
    Ok(())
}

/// `this` on delete: excludes one occurrence (`EXDATE`) and drops its
/// override.
pub fn delete_occurrence(
    event: &mut CalendarEvent,
    recurrence_id: &PimTime,
) -> Result<(), EditError> {
    check_scope(event, EditScope::This, Some(recurrence_id))?;
    let start = event.fields.start.clone();
    let target = frame(recurrence_id, &start)?;
    event
        .overrides
        .retain(|o| time::to_frame(&o.recurrence_id, &start) != Some(target));
    if let Some(recurrence) = event.recurrence.as_mut() {
        let exception = time::like(target, &start);
        if !recurrence.exceptions.contains(&exception) {
            recurrence.exceptions.push(exception);
        }
        // An extra date for that occurrence goes too.
        recurrence
            .dates
            .retain(|d| time::to_frame(d, &start) != Some(target));
    }
    Ok(())
}

/// The last moment before an occurrence, as an `UNTIL` value: a day
/// earlier for dates, a second earlier (in UTC for zoned starts) for
/// times.
fn until_before(target: NaiveDateTime, start: &PimTime) -> Option<PimTime> {
    Some(match start {
        PimTime::Date { .. } => PimTime::date(time::format_date(target.date().pred_opt()?)),
        PimTime::Floating { .. } => {
            PimTime::floating(time::format_local(target - Duration::seconds(1)))
        }
        PimTime::Zoned { tz, .. } => {
            let instant = time::resolve_local(target, time::zone(tz)?) - Duration::seconds(1);
            PimTime::utc(time::format_local(instant.naive_utc()))
        }
    })
}

/// `this-and-following` on delete: the series ends before the
/// occurrence. Returns how many occurrences each rule had generated
/// before it (for [`split_series`]).
pub fn end_before(
    event: &mut CalendarEvent,
    recurrence_id: &PimTime,
) -> Result<Vec<u32>, EditError> {
    check_scope(event, EditScope::ThisAndFollowing, Some(recurrence_id))?;
    let start = event.fields.start.clone();
    let first = time::wall(&start).ok_or(EditError::MalformedTime)?;
    let target = frame(recurrence_id, &start)?;
    if target <= first {
        return Err(EditError::FirstOccurrence);
    }
    let until = until_before(target, &start).ok_or(EditError::MalformedTime)?;
    let mut generated = Vec::new();
    if let Some(recurrence) = event.recurrence.as_mut() {
        for rule in &mut recurrence.rules {
            let bound = match &rule.end {
                RecurrenceEnd::Until { until } => UntilBound::new(until, &start),
                _ => None,
            };
            let before = RuleIter::new(rule, first, start.is_date(), bound)
                .take_while(|o| *o < target)
                .count();
            generated.push(u32::try_from(before).unwrap_or(u32::MAX));
            let ends_earlier = match &rule.end {
                RecurrenceEnd::Until { until: old } => {
                    UntilBound::new(old, &start).is_some_and(|b| b.passed(target))
                }
                RecurrenceEnd::Count { count } => {
                    usize::try_from(*count).is_ok_and(|c| c <= before)
                }
                RecurrenceEnd::Never => false,
            };
            if !ends_earlier {
                rule.end = RecurrenceEnd::Until {
                    until: until.clone(),
                };
            }
        }
        recurrence
            .dates
            .retain(|d| time::to_frame(d, &start).is_some_and(|f| f < target));
        recurrence
            .exceptions
            .retain(|d| time::to_frame(d, &start).is_some_and(|f| f < target));
    }
    event
        .overrides
        .retain(|o| time::to_frame(&o.recurrence_id, &start).is_some_and(|f| f < target));
    Ok(generated)
}

/// `this-and-following` on update: ends the series before the occurrence
/// and returns the new series that starts with it (new id and `UID`, the
/// same calendar, the fields shifted to the occurrence; later
/// `RDATE`s / `EXDATE`s / overrides move over, `COUNT`s are reduced).
/// Apply the edit to the returned series; write it with
/// [`crate::write::WriteOptions::related_to`] = the old `UID`.
pub fn split_series(
    event: &mut CalendarEvent,
    recurrence_id: &PimTime,
    new_id: EventId,
    new_uid: String,
) -> Result<CalendarEvent, EditError> {
    let original = event.clone();
    let start = original.fields.start.clone();
    let target = frame(recurrence_id, &start)?;
    let generated = end_before(event, recurrence_id)?;
    let old = recurrence(&original)?;
    let new_start = time::like(target, &start);
    let span = crate::expand::Span::between(&start, &original.fields.end, Tz::UTC);
    let mut fields = original.fields.clone();
    fields.end = span.end(&new_start, &original.fields.end, Tz::UTC);
    fields.start = new_start;
    let rules = old
        .rules
        .iter()
        .zip(generated)
        .filter_map(|(rule, before)| {
            let mut rule = rule.clone();
            if let RecurrenceEnd::Count { count } = rule.end {
                let left = count.checked_sub(before).filter(|l| *l > 0)?;
                rule.end = RecurrenceEnd::Count { count: left };
            }
            Some(rule)
        })
        .collect();
    let later = |t: &PimTime| time::to_frame(t, &start).is_some_and(|f| f >= target);
    let recurrence = Recurrence {
        rules,
        dates: old.dates.iter().filter(|d| later(d)).cloned().collect(),
        exceptions: old
            .exceptions
            .iter()
            .filter(|d| later(d))
            .cloned()
            .collect(),
    };
    Ok(CalendarEvent {
        id: new_id,
        calendar_id: original.calendar_id.clone(),
        uid: new_uid,
        fields,
        recurrence: Some(recurrence),
        overrides: original
            .overrides
            .iter()
            .filter(|o| later(&o.recurrence_id))
            .cloned()
            .collect(),
        sequence: 0,
        created_at: None,
        updated_at: None,
        read_only: original.read_only,
        sync: PimItemSync::Pending,
    })
}

/// `all` with a new start: the end, recurrence ids, `RDATE`s and
/// `EXDATE`s move by the same wall-clock amount (overrides keep their own
/// times). A change of zone or of all-day-ness re-expresses them in the
/// new start's kind.
pub fn move_series(
    event: &mut CalendarEvent,
    new_start: PimTime,
    new_end: PimTime,
) -> Result<(), EditError> {
    let old_start = event.fields.start.clone();
    let from = time::wall(&old_start).ok_or(EditError::MalformedTime)?;
    let to = time::wall(&new_start).ok_or(EditError::MalformedTime)?;
    let shift = to - from;
    let moved = |t: &PimTime| -> Option<PimTime> {
        let f = time::to_frame(t, &old_start)?;
        Some(time::like(f + shift, &new_start))
    };
    if let Some(recurrence) = event.recurrence.as_mut() {
        recurrence.dates = recurrence.dates.iter().filter_map(moved).collect();
        recurrence.exceptions = recurrence.exceptions.iter().filter_map(moved).collect();
    }
    for o in &mut event.overrides {
        if let Some(rid) = moved(&o.recurrence_id) {
            o.recurrence_id = rid;
        }
    }
    event.fields.start = new_start;
    event.fields.end = new_end;
    Ok(())
}

/// Completing a recurring task: the fields and recurrence of the next
/// occurrence (start and due moved by the same amount, a `COUNT` reduced
/// by one), or `None` when it was the last one.
pub fn advance_task(
    fields: &TaskFields,
    recurrence: &Recurrence,
) -> Option<(TaskFields, Recurrence)> {
    let anchor = fields.start.as_ref().or(fields.due.as_ref())?;
    let mut occurrences = Occurrences::new(anchor, Some(recurrence))?;
    let current = occurrences.next()?;
    let next = occurrences.next()?;
    let shift = next - current;
    let moved = |t: &PimTime| time::wall(t).map(|w| time::like(w + shift, t));
    let mut next_fields = fields.clone();
    next_fields.start = fields.start.as_ref().and_then(moved);
    next_fields.due = fields.due.as_ref().and_then(moved);
    next_fields.status = nail_api::pim::TaskStatus::NeedsAction;
    next_fields.percent_complete = None;
    next_fields.completed_at = None;
    let mut next_recurrence = recurrence.clone();
    for rule in &mut next_recurrence.rules {
        if let RecurrenceEnd::Count { count } = &mut rule.end {
            *count = count.saturating_sub(1).max(1);
        }
    }
    Some((next_fields, next_recurrence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::{ExpandOptions, instances};
    use crate::recur::parse_rrule;
    use pretty_assertions::assert_eq;

    fn series(rule: &str) -> CalendarEvent {
        let start = PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin");
        CalendarEvent {
            id: "e".into(),
            calendar_id: "c".into(),
            uid: "u".into(),
            recurrence: Some(Recurrence::rule(parse_rrule(rule, &start).unwrap())),
            fields: EventFields::new(
                "Standup",
                start,
                PimTime::zoned("2026-10-05T09:15:00", "Europe/Berlin"),
            ),
            overrides: Vec::new(),
            sequence: 2,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    fn berlin(local: &str) -> PimTime {
        PimTime::zoned(local, "Europe/Berlin")
    }

    fn starts(event: &CalendarEvent) -> Vec<String> {
        let options = ExpandOptions::range(
            time::parse_instant("2026-10-01T00:00:00Z").unwrap(),
            time::parse_instant("2027-01-01T00:00:00Z").unwrap(),
            Tz::UTC,
        );
        instances(event, &options)
            .into_iter()
            .map(|i| format!("{} {}", &i.start_utc[5..16], i.title))
            .collect()
    }

    #[test]
    fn scopes_are_checked() {
        let event = series("FREQ=DAILY;COUNT=5");
        assert_eq!(check_scope(&event, EditScope::All, None), Ok(()));
        assert_eq!(
            check_scope(&event, EditScope::This, None),
            Err(EditError::NotAnOccurrence)
        );
        assert_eq!(
            check_scope(
                &event,
                EditScope::This,
                Some(&berlin("2026-10-06T10:00:00"))
            ),
            Err(EditError::NotAnOccurrence)
        );
        assert_eq!(
            check_scope(
                &event,
                EditScope::This,
                Some(&berlin("2026-10-06T09:00:00"))
            ),
            Ok(())
        );
        // A UTC recurrence id of the same instant is the same occurrence.
        assert!(is_occurrence(&event, &PimTime::utc("2026-10-06T07:00:00")));
        assert!(!is_occurrence(&event, &berlin("2026-10-10T09:00:00")));
        let mut single = event.clone();
        single.recurrence = None;
        assert_eq!(
            check_scope(
                &single,
                EditScope::ThisAndFollowing,
                Some(&berlin("2026-10-05T09:00:00"))
            ),
            Err(EditError::NotRecurring)
        );
        assert_eq!(EditError::NotRecurring.issue(), PimIssue::InvalidScope);
    }

    #[test]
    fn this_overrides_and_deletes() {
        let mut event = series("FREQ=DAILY;COUNT=4");
        let mut fields = event.fields.clone();
        fields.title = "Changed".into();
        fields.start = berlin("2026-10-06T11:00:00");
        fields.end = berlin("2026-10-06T11:15:00");
        override_occurrence(&mut event, &PimTime::utc("2026-10-06T07:00:00"), fields).unwrap();
        assert_eq!(
            event.overrides[0].recurrence_id,
            berlin("2026-10-06T09:00:00")
        );
        delete_occurrence(&mut event, &berlin("2026-10-07T09:00:00")).unwrap();
        assert_eq!(
            starts(&event),
            [
                "10-05T07:00 Standup",
                "10-06T09:00 Changed",
                "10-08T07:00 Standup"
            ]
        );
        delete_occurrence(&mut event, &berlin("2026-10-06T09:00:00")).unwrap();
        assert!(event.overrides.is_empty());
        assert_eq!(starts(&event).len(), 2);
    }

    #[test]
    fn this_and_following_splits() {
        let mut event = series("FREQ=DAILY;COUNT=6");
        event.recurrence.as_mut().unwrap().exceptions =
            vec![berlin("2026-10-06T09:00:00"), berlin("2026-10-09T09:00:00")];
        let mut moved = event.fields.clone();
        moved.title = "Moved".into();
        moved.start = berlin("2026-10-08T11:00:00");
        moved.end = berlin("2026-10-08T11:15:00");
        event.overrides.push(EventOverride {
            recurrence_id: berlin("2026-10-08T09:00:00"),
            fields: moved,
            this_and_future: false,
        });
        let before = starts(&event);
        let new = split_series(
            &mut event,
            &berlin("2026-10-08T09:00:00"),
            "e2".into(),
            "u2".into(),
        )
        .unwrap();
        // Together they are what the series was.
        let mut after = starts(&event);
        after.extend(starts(&new));
        assert_eq!(after, before);
        let old_rule = &event.recurrence.as_ref().unwrap().rules[0];
        assert_eq!(
            old_rule.end,
            RecurrenceEnd::Until {
                until: PimTime::utc("2026-10-08T06:59:59")
            }
        );
        assert_eq!(
            event.recurrence.as_ref().unwrap().exceptions,
            [berlin("2026-10-06T09:00:00")]
        );
        assert!(event.overrides.is_empty());
        let new_rule = &new.recurrence.as_ref().unwrap().rules[0];
        assert_eq!(new_rule.end, RecurrenceEnd::Count { count: 3 });
        assert_eq!(new.fields.start, berlin("2026-10-08T09:00:00"));
        assert_eq!(new.fields.end, berlin("2026-10-08T09:15:00"));
        assert_eq!(new.overrides.len(), 1);
        assert_eq!(
            (new.uid.as_str(), new.sequence, new.sync),
            ("u2", 0, PimItemSync::Pending)
        );
        assert_eq!(
            split_series(
                &mut event,
                &berlin("2026-10-05T09:00:00"),
                "e3".into(),
                "u3".into()
            ),
            Err(EditError::FirstOccurrence)
        );
    }

    #[test]
    fn splitting_all_day_and_endless_series() {
        let start = PimTime::date("2026-10-05");
        let mut event = series("FREQ=WEEKLY");
        event.fields.start = start.clone();
        event.fields.end = PimTime::date("2026-10-06");
        event.recurrence = Some(Recurrence::rule(
            parse_rrule("FREQ=WEEKLY", &start).unwrap(),
        ));
        let new = split_series(
            &mut event,
            &PimTime::date("2026-10-19"),
            "e2".into(),
            "u2".into(),
        )
        .unwrap();
        assert_eq!(
            event.recurrence.as_ref().unwrap().rules[0].end,
            RecurrenceEnd::Until {
                until: PimTime::date("2026-10-18")
            }
        );
        assert_eq!(
            new.recurrence.as_ref().unwrap().rules[0].end,
            RecurrenceEnd::Never
        );
        assert_eq!(new.fields.end, PimTime::date("2026-10-20"));
        assert_eq!(starts(&event).len(), 2);
    }

    #[test]
    fn moving_the_whole_series() {
        let mut event = series("FREQ=DAILY;COUNT=3");
        event.recurrence.as_mut().unwrap().exceptions = vec![berlin("2026-10-06T09:00:00")];
        let mut moved = event.fields.clone();
        moved.title = "x".into();
        event.overrides.push(EventOverride {
            recurrence_id: berlin("2026-10-07T09:00:00"),
            fields: moved,
            this_and_future: false,
        });
        move_series(
            &mut event,
            berlin("2026-10-05T14:00:00"),
            berlin("2026-10-05T14:30:00"),
        )
        .unwrap();
        assert_eq!(
            event.recurrence.as_ref().unwrap().exceptions,
            [berlin("2026-10-06T14:00:00")]
        );
        assert_eq!(
            event.overrides[0].recurrence_id,
            berlin("2026-10-07T14:00:00")
        );
        assert_eq!(event.fields.end, berlin("2026-10-05T14:30:00"));
    }

    #[test]
    fn recurring_tasks_move_on() {
        let mut fields = TaskFields::titled("Water plants");
        fields.due = Some(PimTime::date("2026-10-05"));
        fields.status = nail_api::pim::TaskStatus::Completed;
        let recurrence = Recurrence::rule(
            parse_rrule("FREQ=WEEKLY;COUNT=2", &PimTime::date("2026-10-05")).unwrap(),
        );
        let (next, rest) = advance_task(&fields, &recurrence).unwrap();
        assert_eq!(next.due, Some(PimTime::date("2026-10-12")));
        assert_eq!(next.status, nail_api::pim::TaskStatus::NeedsAction);
        assert_eq!(rest.rules[0].end, RecurrenceEnd::Count { count: 1 });
        assert!(advance_task(&next, &rest).is_none(), "COUNT used up");
        assert!(advance_task(&TaskFields::titled("No dates"), &recurrence).is_none());
    }
}
