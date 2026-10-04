//! Exchange events ↔ Nail events (ADR 0072): zones to IANA, recurrence
//! patterns ↔ `RRULE`, cancelled occurrences ↔ `EXDATE`s, exceptions ↔
//! overrides, attendees with their kinds and answers, reminders ↔ alarms,
//! Teams / online-meeting links ↔ conference links, rooms ↔ room
//! attendees.
//!
//! Exchange stores one recurrence pattern per series (daily, weekly,
//! absolute / relative monthly and yearly) with an end date, a count or no
//! end. A Nail series Exchange cannot store ([`event_to_exchange`] answers
//! `exchange-pim-reason-*`) stays local until the user simplifies it.

use chrono::{Datelike, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use nail_api::exchange::pim::{
    AttendeeKind as ExchangeAttendeeKind, EventKind, ExchangeAttendee, ExchangeDateTime,
    ExchangeDayOfWeek, ExchangeEmail, ExchangeEvent, ExchangeLocation, ExchangeRecurrence,
    ExchangeRecurrencePattern, ExchangeRecurrenceRange, ExchangeResponse, PatternKind, Sensitivity,
    ShowAs, WeekIndex,
};
use nail_api::ids::{CalendarId, EventId};
use nail_api::localized::LocalizedMessage;
use nail_api::pim::calendar::{
    AlarmRelated, AttendeeKind, AttendeeRole, EventStatus, Frequency, ParticipationStatus,
    Transparency, Visibility, WeekdayNum,
};
use nail_api::pim::{
    Alarm, AlarmTrigger, Attendee, CalendarEvent, ConferenceLink, EventFields, EventOverride,
    Organizer, PimItemSync, PimTime, Recurrence, RecurrenceEnd, RecurrenceRule,
};
use nail_api::triage::Weekday;

use super::text::{body_text, exchange_body};
use super::time::{exchange_time, instant, like, pim_time, zone_of};
use super::{derived_uid, importance_of, priority_of};
use crate::texts;

/// An Exchange event in Nail's shape (ids are the core's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedEvent {
    /// iCalendar `UID` (the server's, or one derived from the item id).
    pub uid: String,
    /// The series' (or single event's) fields.
    pub fields: EventFields,
    /// The recurrence of a series.
    pub recurrence: Option<Recurrence>,
    /// Changed occurrences.
    pub overrides: Vec<EventOverride>,
    /// `LAST-MODIFIED`.
    pub updated_at: Option<String>,
    /// What did not map exactly.
    pub notes: Vec<LocalizedMessage>,
}

impl MappedEvent {
    /// The event as the store keeps it.
    pub fn into_event(
        self,
        id: EventId,
        calendar_id: CalendarId,
        read_only: bool,
    ) -> CalendarEvent {
        CalendarEvent {
            id,
            calendar_id,
            uid: self.uid,
            fields: self.fields,
            recurrence: self.recurrence,
            overrides: self.overrides,
            sequence: 0,
            created_at: None,
            updated_at: self.updated_at,
            read_only,
            sync: PimItemSync::Synced,
        }
    }
}

fn weekday_of(day: ExchangeDayOfWeek) -> Weekday {
    match day {
        ExchangeDayOfWeek::Monday => Weekday::Mon,
        ExchangeDayOfWeek::Tuesday => Weekday::Tue,
        ExchangeDayOfWeek::Wednesday => Weekday::Wed,
        ExchangeDayOfWeek::Thursday => Weekday::Thu,
        ExchangeDayOfWeek::Friday => Weekday::Fri,
        ExchangeDayOfWeek::Saturday => Weekday::Sat,
        ExchangeDayOfWeek::Sunday => Weekday::Sun,
    }
}

fn exchange_day(day: Weekday) -> ExchangeDayOfWeek {
    match day {
        Weekday::Mon => ExchangeDayOfWeek::Monday,
        Weekday::Tue => ExchangeDayOfWeek::Tuesday,
        Weekday::Wed => ExchangeDayOfWeek::Wednesday,
        Weekday::Thu => ExchangeDayOfWeek::Thursday,
        Weekday::Fri => ExchangeDayOfWeek::Friday,
        Weekday::Sat => ExchangeDayOfWeek::Saturday,
        Weekday::Sun => ExchangeDayOfWeek::Sunday,
    }
}

fn ordinal_of(index: Option<WeekIndex>) -> i8 {
    match index.unwrap_or(WeekIndex::First) {
        WeekIndex::First => 1,
        WeekIndex::Second => 2,
        WeekIndex::Third => 3,
        WeekIndex::Fourth => 4,
        WeekIndex::Last => -1,
    }
}

fn index_of(ordinal: i64) -> Option<WeekIndex> {
    Some(match ordinal {
        1 => WeekIndex::First,
        2 => WeekIndex::Second,
        3 => WeekIndex::Third,
        4 => WeekIndex::Fourth,
        -1 => WeekIndex::Last,
        _ => return None,
    })
}

fn date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()
}

/// The `RRULE` of an Exchange recurrence; `start` is the series start
/// (its kind and zone shape `UNTIL`).
pub fn rrule_of(recurrence: &ExchangeRecurrence, start: &PimTime) -> RecurrenceRule {
    let p = &recurrence.pattern;
    let days: Vec<Weekday> = p.days_of_week.iter().copied().map(weekday_of).collect();
    let mut rule = RecurrenceRule::new(match p.kind {
        PatternKind::Daily => Frequency::Daily,
        PatternKind::Weekly => Frequency::Weekly,
        PatternKind::AbsoluteMonthly | PatternKind::RelativeMonthly => Frequency::Monthly,
        PatternKind::AbsoluteYearly | PatternKind::RelativeYearly => Frequency::Yearly,
    });
    rule.interval = p.interval.max(1);
    let relative = |rule: &mut RecurrenceRule| {
        let ordinal = ordinal_of(p.index);
        if days.len() == 1 {
            rule.by_day = vec![WeekdayNum {
                day: days[0],
                ordinal: Some(ordinal),
            }];
        } else {
            rule.by_day = days
                .iter()
                .map(|d| WeekdayNum {
                    day: *d,
                    ordinal: None,
                })
                .collect();
            rule.by_set_pos = vec![i16::from(ordinal)];
        }
    };
    let month_day = |d: Option<u32>| d.and_then(|d| i8::try_from(d).ok()).into_iter().collect();
    let month = |m: Option<u32>| m.and_then(|m| u8::try_from(m).ok()).into_iter().collect();
    match p.kind {
        PatternKind::Daily => {}
        PatternKind::Weekly => {
            rule.by_day = days
                .iter()
                .map(|d| WeekdayNum {
                    day: *d,
                    ordinal: None,
                })
                .collect();
            rule.week_start = p.first_day_of_week.map(weekday_of);
        }
        PatternKind::AbsoluteMonthly => rule.by_month_day = month_day(p.day_of_month),
        PatternKind::RelativeMonthly => relative(&mut rule),
        PatternKind::AbsoluteYearly => {
            rule.by_month = month(p.month);
            rule.by_month_day = month_day(p.day_of_month);
            // Graph counts yearly intervals in years already.
        }
        PatternKind::RelativeYearly => {
            rule.by_month = month(p.month);
            relative(&mut rule);
        }
    }
    rule.end = match &recurrence.range {
        ExchangeRecurrenceRange::NoEnd { .. } => RecurrenceEnd::Never,
        ExchangeRecurrenceRange::Numbered { occurrences, .. } => RecurrenceEnd::Count {
            count: (*occurrences).max(1),
        },
        ExchangeRecurrenceRange::EndDate { end_date, .. } => match start {
            PimTime::Date { .. } => RecurrenceEnd::Until {
                until: PimTime::date(end_date.clone()),
            },
            PimTime::Floating { .. } => RecurrenceEnd::Until {
                until: PimTime::floating(format!("{end_date}T23:59:59")),
            },
            PimTime::Zoned { tz, .. } => {
                // The last day, to its end, in the series' zone, as UTC.
                let zone = recurrence
                    .time_zone
                    .as_deref()
                    .and_then(zone_of)
                    .or_else(|| zone_of(tz))
                    .unwrap_or(Tz::UTC);
                let last = date(end_date)
                    .map(|d| {
                        d.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap_or(NaiveTime::MIN))
                    })
                    .map(|l| nail_ical::time::resolve_local(l, zone));
                match last {
                    Some(t) => RecurrenceEnd::Until {
                        until: PimTime::utc(t.format("%Y-%m-%dT%H:%M:%S").to_string()),
                    },
                    None => RecurrenceEnd::Never,
                }
            }
        },
    };
    rule
}

fn attendee_of(a: &ExchangeAttendee, rooms: &[String], rsvp: bool) -> Attendee {
    let room = rooms
        .iter()
        .any(|r| r.eq_ignore_ascii_case(&a.email.address));
    let (role, kind) = match a.kind {
        ExchangeAttendeeKind::Required => (AttendeeRole::Required, AttendeeKind::Individual),
        ExchangeAttendeeKind::Optional => (AttendeeRole::Optional, AttendeeKind::Individual),
        ExchangeAttendeeKind::Resource => (
            AttendeeRole::NonParticipant,
            if room {
                AttendeeKind::Room
            } else {
                AttendeeKind::Resource
            },
        ),
    };
    Attendee {
        email: a.email.address.clone(),
        name: a.email.name.clone(),
        role,
        status: match a.response {
            ExchangeResponse::Accepted | ExchangeResponse::Organizer => {
                ParticipationStatus::Accepted
            }
            ExchangeResponse::Declined => ParticipationStatus::Declined,
            ExchangeResponse::TentativelyAccepted => ParticipationStatus::Tentative,
            ExchangeResponse::None | ExchangeResponse::NotResponded => {
                ParticipationStatus::NeedsAction
            }
        },
        kind,
        rsvp,
        delegated_to: Vec::new(),
        delegated_from: Vec::new(),
    }
}

/// The fields of one event or exception.
fn fields_of(e: &ExchangeEvent, notes: &mut Vec<LocalizedMessage>) -> EventFields {
    let (start, start_note) = pim_time(&e.start, e.is_all_day);
    let (end, end_note) = pim_time(&e.end, e.is_all_day);
    notes.extend(start_note);
    notes.extend(end_note);
    let rooms: Vec<String> = e
        .locations
        .iter()
        .filter(|l| l.room)
        .filter_map(|l| l.email.clone())
        .collect();
    let location = e
        .locations
        .iter()
        .map(|l| l.display_name.trim())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let mut fields = EventFields::new(e.subject.clone(), start, end);
    fields.description = body_text(e.body.as_ref());
    fields.location = (!location.is_empty()).then_some(location);
    fields.status = if e.is_cancelled {
        EventStatus::Cancelled
    } else {
        EventStatus::Confirmed
    };
    fields.transparency = if e.show_as == ShowAs::Free {
        Transparency::Free
    } else {
        Transparency::Busy
    };
    fields.visibility = match e.sensitivity {
        Sensitivity::Normal => Visibility::Public,
        Sensitivity::Personal | Sensitivity::Private => Visibility::Private,
        Sensitivity::Confidential => Visibility::Confidential,
    };
    fields.priority = priority_of(e.importance);
    fields.categories.clone_from(&e.categories);
    fields.organizer = e.organizer.as_ref().map(|o| Organizer {
        email: o.address.clone(),
        name: o.name.clone(),
    });
    fields.attendees = e
        .attendees
        .iter()
        .map(|a| attendee_of(a, &rooms, e.response_requested))
        .collect();
    fields.alarms = e
        .reminder_minutes
        .map(Alarm::minutes_before)
        .into_iter()
        .collect();
    fields.conferences = e
        .online_meeting_url
        .iter()
        .map(|uri| ConferenceLink {
            uri: uri.clone(),
            label: None,
            features: Vec::new(),
        })
        .collect();
    fields
}

/// The original start of an occurrence in the series start's kind and
/// zone.
fn recurrence_id(original: &ExchangeDateTime, series_start: &PimTime) -> Option<PimTime> {
    if series_start.is_date() {
        return Some(PimTime::date(original.date()));
    }
    instant(original).map(|t| like(t, series_start))
}

/// An Exchange event (a series with its exceptions, or a single event) in
/// Nail's shape.
pub fn event_from_exchange(e: &ExchangeEvent) -> MappedEvent {
    let mut notes = Vec::new();
    let fields = fields_of(e, &mut notes);
    let recurrence = e.recurrence.as_ref().map(|r| {
        let start = &fields.start;
        let exceptions = e
            .cancelled_occurrences
            .iter()
            .filter_map(|d| date(d))
            .map(|d| match start {
                PimTime::Date { .. } => PimTime::date(d.format("%Y-%m-%d").to_string()),
                PimTime::Floating { local } | PimTime::Zoned { local, .. } => {
                    let time = local.get(10..).unwrap_or("T00:00:00");
                    let local = format!("{}{time}", d.format("%Y-%m-%d"));
                    match start {
                        PimTime::Zoned { tz, .. } => PimTime::zoned(local, tz.clone()),
                        _ => PimTime::floating(local),
                    }
                }
            })
            .collect();
        Recurrence {
            rules: vec![rrule_of(r, start)],
            dates: Vec::new(),
            exceptions,
        }
    });
    let overrides = e
        .exceptions
        .iter()
        .filter_map(|x| {
            let original = x.original_start.as_ref()?;
            Some(EventOverride {
                recurrence_id: recurrence_id(original, &fields.start)?,
                fields: fields_of(x, &mut notes),
                this_and_future: false,
            })
        })
        .collect();
    MappedEvent {
        uid: e
            .uid
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| derived_uid("event", &e.remote_id)),
        fields,
        recurrence,
        overrides,
        updated_at: e.last_modified.clone(),
        notes,
    }
}

/// Why a Nail event cannot be written to Exchange as it is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not expressible in Exchange: {}", .0.id)]
pub struct NotExpressible(pub LocalizedMessage);

fn recurrence_error() -> NotExpressible {
    NotExpressible(LocalizedMessage::new(texts::PIM_RECURRENCE))
}

fn start_date(start: &PimTime) -> Option<NaiveDate> {
    date(start.date_part())
}

/// The Exchange pattern of a Nail `RRULE` (see the module docs).
pub fn pattern_of(
    rule: &RecurrenceRule,
    start: &PimTime,
) -> Result<ExchangeRecurrence, NotExpressible> {
    let first = start_date(start).ok_or_else(recurrence_error)?;
    let unsupported = !rule.by_second.is_empty()
        || !rule.by_minute.is_empty()
        || !rule.by_hour.is_empty()
        || !rule.by_year_day.is_empty()
        || !rule.by_week_no.is_empty()
        || rule.by_set_pos.len() > 1
        || rule.by_month.len() > 1
        || rule.by_month_day.len() > 1
        || !rule.is_valid();
    if unsupported {
        return Err(recurrence_error());
    }
    let start_day = nail_ical::recur::api_weekday(first.weekday());
    let plain_days: Option<Vec<ExchangeDayOfWeek>> = rule
        .by_day
        .iter()
        .map(|d| d.ordinal.is_none().then(|| exchange_day(d.day)))
        .collect();
    let base = |kind: PatternKind| ExchangeRecurrencePattern {
        kind,
        interval: rule.interval.max(1),
        days_of_week: Vec::new(),
        day_of_month: None,
        month: None,
        index: None,
        first_day_of_week: None,
    };
    // A relative pattern: one `BYDAY` with an ordinal, or plain days with
    // one `BYSETPOS`.
    let relative = || -> Option<(Vec<ExchangeDayOfWeek>, WeekIndex)> {
        match (rule.by_day.as_slice(), rule.by_set_pos.as_slice()) {
            ([one], []) => Some((vec![exchange_day(one.day)], index_of(one.ordinal?.into())?)),
            (days, [pos]) if !days.is_empty() => {
                Some((plain_days.clone()?, index_of((*pos).into())?))
            }
            _ => None,
        }
    };
    let month_day = rule.by_month_day.first().map_or(Some(first.day()), |d| {
        u32::try_from(*d).ok().filter(|d| *d >= 1)
    });
    let pattern = match rule.freq {
        Frequency::Daily
            if rule.by_day.is_empty()
                && rule.by_month_day.is_empty()
                && rule.by_month.is_empty() =>
        {
            base(PatternKind::Daily)
        }
        // "Every weekday" is written as a daily rule with days.
        Frequency::Daily | Frequency::Weekly
            if rule.by_month_day.is_empty()
                && rule.by_month.is_empty()
                && rule.by_set_pos.is_empty()
                && (rule.freq == Frequency::Weekly || rule.interval == 1)
                && plain_days.is_some() =>
        {
            let mut days = plain_days.clone().unwrap_or_default();
            if days.is_empty() {
                days.push(exchange_day(start_day));
            }
            ExchangeRecurrencePattern {
                days_of_week: days,
                first_day_of_week: Some(exchange_day(rule.week_start.unwrap_or(Weekday::Mon))),
                ..base(PatternKind::Weekly)
            }
        }
        Frequency::Monthly if rule.by_month.is_empty() => {
            if rule.by_day.is_empty() {
                ExchangeRecurrencePattern {
                    day_of_month: Some(month_day.ok_or_else(recurrence_error)?),
                    ..base(PatternKind::AbsoluteMonthly)
                }
            } else if rule.by_month_day.is_empty() {
                let (days, index) = relative().ok_or_else(recurrence_error)?;
                ExchangeRecurrencePattern {
                    days_of_week: days,
                    index: Some(index),
                    ..base(PatternKind::RelativeMonthly)
                }
            } else {
                return Err(recurrence_error());
            }
        }
        Frequency::Yearly => {
            let month = rule
                .by_month
                .first()
                .map_or(first.month(), |m| u32::from(*m));
            if rule.by_day.is_empty() {
                ExchangeRecurrencePattern {
                    month: Some(month),
                    day_of_month: Some(month_day.ok_or_else(recurrence_error)?),
                    ..base(PatternKind::AbsoluteYearly)
                }
            } else if rule.by_month_day.is_empty() && !rule.by_month.is_empty() {
                let (days, index) = relative().ok_or_else(recurrence_error)?;
                ExchangeRecurrencePattern {
                    month: Some(month),
                    days_of_week: days,
                    index: Some(index),
                    ..base(PatternKind::RelativeYearly)
                }
            } else {
                return Err(recurrence_error());
            }
        }
        _ => return Err(recurrence_error()),
    };
    let start_date = first.format("%Y-%m-%d").to_string();
    let zone = start.tz().and_then(zone_of);
    let range = match &rule.end {
        RecurrenceEnd::Never => ExchangeRecurrenceRange::NoEnd { start_date },
        RecurrenceEnd::Count { count } => ExchangeRecurrenceRange::Numbered {
            start_date,
            occurrences: *count,
        },
        RecurrenceEnd::Until { until } => {
            // The date of the last possible start, in the series' zone.
            let end = match (until, zone) {
                (PimTime::Zoned { .. }, Some(zone)) => nail_ical::time::instant(until, zone)
                    .map(|t| zone.from_utc_datetime(&t.naive_utc()).date_naive()),
                _ => date(until.date_part()),
            }
            .ok_or_else(recurrence_error)?;
            ExchangeRecurrenceRange::EndDate {
                start_date,
                end_date: end.format("%Y-%m-%d").to_string(),
            }
        }
    };
    Ok(ExchangeRecurrence {
        pattern,
        range,
        time_zone: zone.map(|z| z.name().to_owned()),
    })
}

/// What [`event_to_exchange`] needs besides the event.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext<'a> {
    /// The item's server id and version (`None` for a new item).
    pub remote: Option<(&'a str, Option<&'a str>)>,
    /// The mailbox's zone for dates and floating times (IANA or Windows).
    pub default_zone: &'a str,
    /// The user's own addresses (organizer / own answer).
    pub own: &'a [String],
}

fn exchange_attendee(a: &Attendee) -> ExchangeAttendee {
    ExchangeAttendee {
        email: ExchangeEmail {
            address: a.email.clone(),
            name: a.name.clone(),
        },
        kind: match (a.kind, a.role) {
            (AttendeeKind::Room | AttendeeKind::Resource, _) => ExchangeAttendeeKind::Resource,
            (_, AttendeeRole::Optional | AttendeeRole::NonParticipant) => {
                ExchangeAttendeeKind::Optional
            }
            _ => ExchangeAttendeeKind::Required,
        },
        response: response_of(a.status),
        proposed: None,
    }
}

fn response_of(status: ParticipationStatus) -> ExchangeResponse {
    match status {
        ParticipationStatus::Accepted => ExchangeResponse::Accepted,
        ParticipationStatus::Declined => ExchangeResponse::Declined,
        ParticipationStatus::Tentative => ExchangeResponse::TentativelyAccepted,
        ParticipationStatus::NeedsAction | ParticipationStatus::Delegated => {
            ExchangeResponse::NotResponded
        }
    }
}

fn exchange_fields(
    fields: &EventFields,
    ctx: &WriteContext<'_>,
    notes: &mut Vec<LocalizedMessage>,
) -> ExchangeEvent {
    let (start, n1) = exchange_time(&fields.start, ctx.default_zone);
    let (end, n2) = exchange_time(&fields.end, ctx.default_zone);
    notes.extend(n1);
    notes.extend(n2);
    let own = |email: &str| ctx.own.iter().any(|o| o.eq_ignore_ascii_case(email));
    let is_organizer = fields.organizer.as_ref().is_none_or(|o| own(&o.email));
    let mut locations: Vec<ExchangeLocation> = fields
        .location
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| ExchangeLocation {
            display_name: l.clone(),
            email: None,
            room: false,
        })
        .collect();
    for room in fields
        .attendees
        .iter()
        .filter(|a| a.kind == AttendeeKind::Room)
    {
        locations.push(ExchangeLocation {
            display_name: room.name.clone().unwrap_or_else(|| room.email.clone()),
            email: Some(room.email.clone()),
            room: true,
        });
    }
    let reminder_minutes = fields.alarms.iter().find_map(|a| match &a.trigger {
        AlarmTrigger::Relative {
            offset_secs,
            related: AlarmRelated::Start,
        } if *offset_secs <= 0 => u32::try_from(-offset_secs / 60).ok(),
        _ => None,
    });
    ExchangeEvent {
        remote_id: ctx.remote.map(|(id, _)| id.to_owned()).unwrap_or_default(),
        change_key: ctx.remote.and_then(|(_, k)| k.map(str::to_owned)),
        uid: None,
        kind: EventKind::SingleInstance,
        series_master_id: None,
        original_start: None,
        subject: fields.title.clone(),
        body: exchange_body(fields.description.as_deref()),
        start,
        end,
        is_all_day: fields.is_all_day(),
        locations,
        show_as: if fields.transparency == Transparency::Free {
            ShowAs::Free
        } else {
            ShowAs::Busy
        },
        sensitivity: match fields.visibility {
            Visibility::Public => Sensitivity::Normal,
            Visibility::Private => Sensitivity::Private,
            Visibility::Confidential => Sensitivity::Confidential,
        },
        importance: importance_of(fields.priority),
        reminder_minutes,
        organizer: fields.organizer.as_ref().map(|o| ExchangeEmail {
            address: o.email.clone(),
            name: o.name.clone(),
        }),
        attendees: fields
            .attendees
            .iter()
            .filter(|a| {
                fields
                    .organizer
                    .as_ref()
                    .is_none_or(|o| !o.email.eq_ignore_ascii_case(&a.email))
            })
            .map(exchange_attendee)
            .collect(),
        is_organizer,
        response_requested: fields.attendees.iter().any(|a| a.rsvp),
        my_response: if is_organizer {
            ExchangeResponse::Organizer
        } else {
            fields
                .attendees
                .iter()
                .find(|a| own(&a.email))
                .map_or(ExchangeResponse::None, |a| response_of(a.status))
        },
        is_cancelled: fields.status == EventStatus::Cancelled,
        online_meeting_url: fields.conferences.first().map(|c| c.uri.clone()),
        categories: fields.categories.clone(),
        recurrence: None,
        cancelled_occurrences: Vec::new(),
        exceptions: Vec::new(),
        last_modified: None,
    }
}

/// A Nail event for Exchange (series with pattern, cancelled occurrences
/// and exceptions). Errors name what Exchange cannot store; notes what
/// changed on the way (floating times).
pub fn event_to_exchange(
    event: &CalendarEvent,
    ctx: &WriteContext<'_>,
) -> Result<(ExchangeEvent, Vec<LocalizedMessage>), NotExpressible> {
    let mut notes = Vec::new();
    let mut out = exchange_fields(&event.fields, ctx, &mut notes);
    out.uid = Some(event.uid.clone());
    if let Some(recurrence) = event.recurrence.as_ref().filter(|r| !r.is_empty()) {
        if recurrence.rules.len() != 1 || !recurrence.dates.is_empty() {
            return Err(NotExpressible(LocalizedMessage::new(texts::PIM_RDATE)));
        }
        out.kind = EventKind::SeriesMaster;
        out.recurrence = Some(pattern_of(&recurrence.rules[0], &event.fields.start)?);
        out.cancelled_occurrences = recurrence
            .exceptions
            .iter()
            .map(|t| t.date_part().to_owned())
            .filter(|d| !d.is_empty())
            .collect();
        for o in &event.overrides {
            if o.this_and_future {
                return Err(recurrence_error());
            }
            if o.fields.status == EventStatus::Cancelled {
                out.cancelled_occurrences
                    .push(o.recurrence_id.date_part().to_owned());
                continue;
            }
            let mut x = exchange_fields(
                &o.fields,
                &WriteContext {
                    remote: None,
                    ..*ctx
                },
                &mut notes,
            );
            x.kind = EventKind::Exception;
            x.original_start = Some(exchange_time(&o.recurrence_id, ctx.default_zone).0);
            out.exceptions.push(x);
        }
        out.cancelled_occurrences.sort();
        out.cancelled_occurrences.dedup();
    }
    Ok((out, notes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn berlin(local: &str) -> ExchangeDateTime {
        ExchangeDateTime {
            date_time: local.into(),
            time_zone: "W. Europe Standard Time".into(),
        }
    }

    fn weekly_series() -> ExchangeEvent {
        ExchangeEvent {
            remote_id: "AAMkAD".into(),
            change_key: Some("DwAAABY".into()),
            uid: Some("040000008200E00074C5B7101A82E008".into()),
            kind: EventKind::SeriesMaster,
            series_master_id: None,
            original_start: None,
            subject: "Weekly sync".into(),
            body: Some(nail_api::exchange::pim::ExchangeBody {
                html: true,
                content: "<p>Agenda</p>".into(),
            }),
            start: berlin("2026-10-05T09:00:00"),
            end: berlin("2026-10-05T09:30:00"),
            is_all_day: false,
            locations: vec![ExchangeLocation {
                display_name: "Room 1".into(),
                email: Some("room1@corp.example".into()),
                room: true,
            }],
            show_as: ShowAs::Busy,
            sensitivity: Sensitivity::Private,
            importance: Some(nail_api::exchange::rules::Importance::High),
            reminder_minutes: Some(15),
            organizer: Some(ExchangeEmail {
                address: "ada@corp.example".into(),
                name: Some("Ada".into()),
            }),
            attendees: vec![
                ExchangeAttendee {
                    email: ExchangeEmail {
                        address: "bob@corp.example".into(),
                        name: None,
                    },
                    kind: ExchangeAttendeeKind::Required,
                    response: ExchangeResponse::Accepted,
                    proposed: None,
                },
                ExchangeAttendee {
                    email: ExchangeEmail {
                        address: "room1@corp.example".into(),
                        name: Some("Room 1".into()),
                    },
                    kind: ExchangeAttendeeKind::Resource,
                    response: ExchangeResponse::Accepted,
                    proposed: None,
                },
            ],
            is_organizer: true,
            response_requested: true,
            my_response: ExchangeResponse::Organizer,
            is_cancelled: false,
            online_meeting_url: Some("https://teams.example/l/meetup".into()),
            categories: vec!["Work".into()],
            recurrence: Some(ExchangeRecurrence {
                pattern: ExchangeRecurrencePattern {
                    kind: PatternKind::Weekly,
                    interval: 1,
                    days_of_week: vec![ExchangeDayOfWeek::Monday],
                    day_of_month: None,
                    month: None,
                    index: None,
                    first_day_of_week: Some(ExchangeDayOfWeek::Monday),
                },
                range: ExchangeRecurrenceRange::EndDate {
                    start_date: "2026-10-05".into(),
                    end_date: "2026-12-28".into(),
                },
                time_zone: Some("W. Europe Standard Time".into()),
            }),
            cancelled_occurrences: vec!["2026-10-12".into()],
            exceptions: vec![ExchangeEvent {
                remote_id: "AAMkAD-x".into(),
                kind: EventKind::Exception,
                original_start: Some(ExchangeDateTime::utc("2026-10-19T07:00:00")),
                start: berlin("2026-10-19T10:00:00"),
                end: berlin("2026-10-19T10:30:00"),
                subject: "Weekly sync (moved)".into(),
                recurrence: None,
                cancelled_occurrences: Vec::new(),
                exceptions: Vec::new(),
                ..weekly_master_stub()
            }],
            last_modified: Some("2026-10-01T08:00:00Z".into()),
        }
    }

    fn weekly_master_stub() -> ExchangeEvent {
        ExchangeEvent {
            remote_id: String::new(),
            change_key: None,
            uid: None,
            kind: EventKind::SingleInstance,
            series_master_id: None,
            original_start: None,
            subject: String::new(),
            body: None,
            start: ExchangeDateTime::utc("2026-01-01T00:00:00"),
            end: ExchangeDateTime::utc("2026-01-01T00:00:00"),
            is_all_day: false,
            locations: Vec::new(),
            show_as: ShowAs::Busy,
            sensitivity: Sensitivity::Normal,
            importance: None,
            reminder_minutes: None,
            organizer: None,
            attendees: Vec::new(),
            is_organizer: false,
            response_requested: false,
            my_response: ExchangeResponse::None,
            is_cancelled: false,
            online_meeting_url: None,
            categories: Vec::new(),
            recurrence: None,
            cancelled_occurrences: Vec::new(),
            exceptions: Vec::new(),
            last_modified: None,
        }
    }

    #[test]
    fn a_weekly_series_maps_to_nail() {
        let mapped = event_from_exchange(&weekly_series());
        assert!(mapped.notes.is_empty(), "{:?}", mapped.notes);
        assert_eq!(mapped.uid, "040000008200E00074C5B7101A82E008");
        let f = &mapped.fields;
        assert_eq!(
            f.start,
            PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin")
        );
        assert_eq!(f.description.as_deref(), Some("Agenda"));
        assert_eq!(f.location.as_deref(), Some("Room 1"));
        assert_eq!(f.visibility, Visibility::Private);
        assert_eq!(f.priority, Some(1));
        assert_eq!(f.alarms, [Alarm::minutes_before(15)]);
        assert_eq!(f.conferences[0].uri, "https://teams.example/l/meetup");
        assert_eq!(f.organizer.as_ref().unwrap().email, "ada@corp.example");
        assert_eq!(f.attendees[0].status, ParticipationStatus::Accepted);
        assert!(f.attendees[0].rsvp);
        assert_eq!(f.attendees[1].kind, AttendeeKind::Room);
        assert_eq!(f.attendees[1].role, AttendeeRole::NonParticipant);
        let recurrence = mapped.recurrence.as_ref().unwrap();
        let rule = &recurrence.rules[0];
        assert_eq!(rule.freq, Frequency::Weekly);
        assert_eq!(
            rule.by_day,
            [WeekdayNum {
                day: Weekday::Mon,
                ordinal: None
            }]
        );
        assert_eq!(
            rule.end,
            RecurrenceEnd::Until {
                until: PimTime::utc("2026-12-28T22:59:59")
            }
        );
        assert_eq!(
            recurrence.exceptions,
            [PimTime::zoned("2026-10-12T09:00:00", "Europe/Berlin")]
        );
        assert_eq!(mapped.overrides.len(), 1);
        assert_eq!(
            mapped.overrides[0].recurrence_id,
            PimTime::zoned("2026-10-19T09:00:00", "Europe/Berlin")
        );
        assert_eq!(mapped.overrides[0].fields.title, "Weekly sync (moved)");
        let event = mapped.into_event("e1".into(), "c1".into(), false);
        assert!(event.is_recurring());
        assert_eq!(event.updated_at.as_deref(), Some("2026-10-01T08:00:00Z"));
    }

    #[test]
    fn a_series_round_trips_to_exchange() {
        let source = weekly_series();
        let event = event_from_exchange(&source).into_event("e1".into(), "c1".into(), false);
        let own = vec!["ada@corp.example".to_owned()];
        let ctx = WriteContext {
            remote: Some(("AAMkAD", Some("DwAAABY"))),
            default_zone: "Europe/Berlin",
            own: &own,
        };
        let (back, notes) = event_to_exchange(&event, &ctx).unwrap();
        assert!(notes.is_empty());
        assert_eq!(back.remote_id, "AAMkAD");
        assert_eq!(back.change_key.as_deref(), Some("DwAAABY"));
        assert_eq!(back.kind, EventKind::SeriesMaster);
        assert!(back.is_organizer);
        assert_eq!(back.my_response, ExchangeResponse::Organizer);
        let pattern = back.recurrence.as_ref().unwrap();
        assert_eq!(pattern.pattern, source.recurrence.as_ref().unwrap().pattern);
        assert_eq!(pattern.range, source.recurrence.as_ref().unwrap().range);
        assert_eq!(back.cancelled_occurrences, ["2026-10-12"]);
        assert_eq!(back.exceptions.len(), 1);
        assert_eq!(
            back.exceptions[0].original_start,
            Some(ExchangeDateTime {
                date_time: "2026-10-19T09:00:00".into(),
                time_zone: "Europe/Berlin".into()
            })
        );
        assert_eq!(back.reminder_minutes, Some(15));
        assert_eq!(
            back.importance,
            Some(nail_api::exchange::rules::Importance::High)
        );
        assert_eq!(back.sensitivity, Sensitivity::Private);
        assert_eq!(back.online_meeting_url, source.online_meeting_url);
        assert!(back.response_requested);
        assert_eq!(back.attendees.len(), 2);
        assert_eq!(back.attendees[1].kind, ExchangeAttendeeKind::Resource);
        assert!(
            back.locations
                .iter()
                .any(|l| l.room && l.email.as_deref() == Some("room1@corp.example"))
        );
    }

    fn rule(text: &str, start: &PimTime) -> RecurrenceRule {
        nail_ical::recur::parse_rrule(text, start).unwrap()
    }

    #[test]
    fn rrules_to_patterns() {
        let start = PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin"); // a Monday
        let pattern = |text: &str| pattern_of(&rule(text, &start), &start).map(|r| r.pattern);
        let p = pattern("FREQ=DAILY;INTERVAL=2;COUNT=5").unwrap();
        assert_eq!((p.kind, p.interval), (PatternKind::Daily, 2));
        let p = pattern("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR").unwrap();
        assert_eq!(p.kind, PatternKind::Weekly);
        assert_eq!(p.days_of_week.len(), 5);
        let p = pattern("FREQ=WEEKLY").unwrap();
        assert_eq!(p.days_of_week, [ExchangeDayOfWeek::Monday]);
        let p = pattern("FREQ=MONTHLY").unwrap();
        assert_eq!(
            (p.kind, p.day_of_month),
            (PatternKind::AbsoluteMonthly, Some(5))
        );
        let p = pattern("FREQ=MONTHLY;BYDAY=-1FR").unwrap();
        assert_eq!(p.kind, PatternKind::RelativeMonthly);
        assert_eq!(p.index, Some(WeekIndex::Last));
        assert_eq!(p.days_of_week, [ExchangeDayOfWeek::Friday]);
        let p = pattern("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1").unwrap();
        assert_eq!(
            (p.kind, p.index),
            (PatternKind::RelativeMonthly, Some(WeekIndex::First))
        );
        let p = pattern("FREQ=YEARLY").unwrap();
        assert_eq!(
            (p.kind, p.month, p.day_of_month),
            (PatternKind::AbsoluteYearly, Some(10), Some(5))
        );
        let p = pattern("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH").unwrap();
        assert_eq!(
            (p.kind, p.month, p.index),
            (
                PatternKind::RelativeYearly,
                Some(11),
                Some(WeekIndex::Fourth)
            )
        );
        for bad in [
            "FREQ=HOURLY",
            "FREQ=MONTHLY;BYMONTHDAY=-1",
            "FREQ=MONTHLY;BYDAY=2MO,3MO",
            "FREQ=YEARLY;BYWEEKNO=20",
            "FREQ=MONTHLY;BYMONTHDAY=1,15",
            "FREQ=DAILY;INTERVAL=2;BYDAY=MO",
        ] {
            assert_eq!(
                pattern(bad).unwrap_err().0.id,
                texts::PIM_RECURRENCE,
                "{bad}"
            );
        }
    }

    #[test]
    fn patterns_round_trip_through_rrules() {
        let start = PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin");
        for text in [
            "FREQ=DAILY;INTERVAL=3;COUNT=10",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=SU",
            "FREQ=MONTHLY;BYMONTHDAY=5",
            "FREQ=MONTHLY;INTERVAL=2;BYDAY=2TU",
            "FREQ=YEARLY;BYMONTH=10;BYMONTHDAY=5",
            "FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU",
            "FREQ=WEEKLY;BYDAY=MO;UNTIL=20261228T225959Z",
        ] {
            let original = rule(text, &start);
            let exchange = pattern_of(&original, &start).unwrap();
            let back = rrule_of(&exchange, &start);
            let again = pattern_of(&back, &start).unwrap();
            assert_eq!(again, exchange, "{text}");
        }
    }

    #[test]
    fn all_day_and_other_shapes() {
        let mut single = weekly_master_stub();
        single.remote_id = "AAMkAll".into();
        single.is_all_day = true;
        single.start = berlin("2026-10-05T00:00:00");
        single.end = berlin("2026-10-06T00:00:00");
        single.show_as = ShowAs::Free;
        single.is_cancelled = true;
        let mapped = event_from_exchange(&single);
        assert_eq!(mapped.fields.start, PimTime::date("2026-10-05"));
        assert_eq!(mapped.fields.end, PimTime::date("2026-10-06"));
        assert_eq!(mapped.fields.transparency, Transparency::Free);
        assert_eq!(mapped.fields.status, EventStatus::Cancelled);
        assert!(mapped.uid.starts_with("event-"), "derived uid");
        assert!(mapped.recurrence.is_none());

        let event = mapped.into_event("e2".into(), "c1".into(), false);
        let ctx = WriteContext {
            remote: None,
            default_zone: "Europe/Berlin",
            own: &[],
        };
        let (back, _) = event_to_exchange(&event, &ctx).unwrap();
        assert!(back.is_all_day);
        assert_eq!(back.start.date_time, "2026-10-05T00:00:00");
        assert_eq!(back.show_as, ShowAs::Free);
        assert!(back.is_cancelled);
        assert!(back.is_organizer, "no organizer = the user's own event");

        // Several rules cannot be stored.
        let mut event = event;
        event.fields.start = PimTime::zoned("2026-10-05T09:00:00", "Europe/Berlin");
        event.fields.end = PimTime::zoned("2026-10-05T10:00:00", "Europe/Berlin");
        let daily = rule("FREQ=DAILY", &event.fields.start);
        event.recurrence = Some(Recurrence {
            rules: vec![daily.clone(), daily],
            ..Recurrence::default()
        });
        assert_eq!(
            event_to_exchange(&event, &ctx).unwrap_err().0.id,
            texts::PIM_RDATE
        );
    }

    #[test]
    fn unknown_zones_and_answers() {
        let mut e = weekly_master_stub();
        e.remote_id = "x".into();
        e.start = ExchangeDateTime {
            date_time: "2026-10-05T09:00:00".into(),
            time_zone: "Customized Time Zone".into(),
        };
        e.end = e.start.clone();
        e.attendees = vec![ExchangeAttendee {
            email: ExchangeEmail {
                address: "me@corp.example".into(),
                name: None,
            },
            kind: ExchangeAttendeeKind::Optional,
            response: ExchangeResponse::TentativelyAccepted,
            proposed: None,
        }];
        e.organizer = Some(ExchangeEmail {
            address: "boss@corp.example".into(),
            name: None,
        });
        let mapped = event_from_exchange(&e);
        assert_eq!(mapped.notes.len(), 2, "start and end");
        assert_eq!(mapped.notes[0].id, texts::PIM_ZONE);
        assert_eq!(
            mapped.fields.attendees[0].status,
            ParticipationStatus::Tentative
        );
        assert_eq!(mapped.fields.attendees[0].role, AttendeeRole::Optional);
        let own = vec!["ME@corp.example".to_owned()];
        let event = mapped.into_event("e".into(), "c".into(), false);
        let (back, _) = event_to_exchange(
            &event,
            &WriteContext {
                remote: None,
                default_zone: "UTC",
                own: &own,
            },
        )
        .unwrap();
        assert!(!back.is_organizer);
        assert_eq!(back.my_response, ExchangeResponse::TentativelyAccepted);
    }
}
