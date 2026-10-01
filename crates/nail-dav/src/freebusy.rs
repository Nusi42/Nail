//! Free/busy: `VFREEBUSY` building and parsing (RFC 5545 §3.6.4,
//! RFC 5546 §3.3, RFC 6638 §5) and the busy periods of the user's own
//! calendars from expanded event instances (M3b WP83; contract ADR 0065).
//!
//! All times here are UTC. The contract's times are RFC 3339
//! (`YYYY-MM-DDTHH:MM:SSZ`); iCalendar's are `YYYYMMDDTHHMMSSZ`. Periods
//! come out clipped to the asked range, with whole seconds, and merged
//! with [`BusyPeriod::merge`].

use nail_api::ids::{CalendarId, EventId};
use nail_api::pim::calendar::{EventInstance, EventStatus, ParticipationStatus, Transparency};
use nail_api::scheduling::{BusyKind, BusyPeriod, FreeBusyQuery, utc_seconds};

use crate::icalendar::{ContentLine, address_of};
use crate::ops::utc_stamp;

/// The `PRODID` Nail writes.
pub const PRODID: &str = "-//Nail//Nail Scheduling//EN";

/// Most periods read from one `VFREEBUSY` (a hostile server cannot make
/// Nail allocate without bound).
pub const MAX_PERIODS: usize = 20_000;

// ---------------------------------------------------------------------------
// Time helpers

/// Unix seconds → `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(secs: i64) -> String {
    let s = utc_stamp(secs);
    format!(
        "{}-{}-{}T{}:{}:{}Z",
        &s[0..4],
        &s[4..6],
        &s[6..8],
        &s[9..11],
        &s[11..13],
        &s[13..15]
    )
}

/// An iCalendar `DATE-TIME` (`YYYYMMDDTHHMMSS`, `Z` optional: free/busy
/// times are UTC by definition) or `DATE` (`YYYYMMDD`, midnight UTC) →
/// Unix seconds.
pub fn ical_seconds(value: &str) -> Option<i64> {
    let v = value.trim();
    let v = v.strip_suffix(['Z', 'z']).unwrap_or(v);
    let b = v.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    let text = match b.len() {
        8 if digits(0..8) => format!("{}-{}-{}T00:00:00Z", &v[0..4], &v[4..6], &v[6..8]),
        15 if digits(0..8) && b[8].eq_ignore_ascii_case(&b'T') && digits(9..15) => format!(
            "{}-{}-{}T{}:{}:{}Z",
            &v[0..4],
            &v[4..6],
            &v[6..8],
            &v[9..11],
            &v[11..13],
            &v[13..15]
        ),
        _ => return None,
    };
    utc_seconds(&text)
}

/// An RFC 5545 `DURATION` (`P1W`, `P1DT2H`, `PT30M`, `+PT1H`) in seconds;
/// negative durations are `None` (free/busy periods run forward).
pub fn duration_seconds(value: &str) -> Option<i64> {
    let v = value.trim();
    let v = v.strip_prefix('+').unwrap_or(v);
    let rest = v.strip_prefix(['P', 'p'])?;
    if rest.is_empty() {
        return None;
    }
    let mut total: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => number.push(c),
            'T' if number.is_empty() && !in_time => in_time = true,
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                let factor = match (unit, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
                total = total.checked_add(n.checked_mul(factor)?)?;
                any = true;
            }
        }
    }
    (number.is_empty() && any).then_some(total)
}

/// `YYYY-MM-DDTHH:MM:SS[.f]Z` → iCalendar `YYYYMMDDTHHMMSSZ`.
pub fn ical_stamp(rfc3339: &str) -> Option<String> {
    utc_seconds(rfc3339).map(utc_stamp)
}

// ---------------------------------------------------------------------------
// Building

/// `FBTYPE` text of a busy kind.
pub fn fbtype(kind: BusyKind) -> &'static str {
    match kind {
        BusyKind::Busy => "BUSY",
        BusyKind::Tentative => "BUSY-TENTATIVE",
        BusyKind::Unavailable => "BUSY-UNAVAILABLE",
    }
}

/// The busy kind of an `FBTYPE` value: `None` for `FREE`; unknown values
/// count as `BUSY` (RFC 5545 §3.2.9).
pub fn busy_kind(fbtype: Option<&str>) -> Option<BusyKind> {
    match fbtype.map(|t| t.trim().to_ascii_uppercase()).as_deref() {
        Some("FREE") => None,
        Some("BUSY-TENTATIVE") => Some(BusyKind::Tentative),
        Some("BUSY-UNAVAILABLE") => Some(BusyKind::Unavailable),
        _ => Some(BusyKind::Busy),
    }
}

fn mailto(address: &str) -> String {
    format!("mailto:{}", address_of(address))
}

/// A `VFREEBUSY` request (`METHOD:REQUEST`) for the CalDAV schedule
/// outbox (RFC 6638 §5) or iMIP. `None` when the range is not RFC 3339
/// UTC or empty.
pub fn build_request(query: &FreeBusyQuery, uid: &str, dtstamp_unix: i64) -> Option<String> {
    let start = utc_seconds(&query.from)?;
    let end = utc_seconds(&query.to)?;
    if end <= start {
        return None;
    }
    let mut out = String::new();
    for line in [
        ContentLine::new("BEGIN", "VCALENDAR"),
        ContentLine::new("VERSION", "2.0"),
        ContentLine::new("PRODID", PRODID),
        ContentLine::new("METHOD", "REQUEST"),
        ContentLine::new("BEGIN", "VFREEBUSY"),
        ContentLine::new("UID", uid),
        ContentLine::new("DTSTAMP", utc_stamp(dtstamp_unix)),
        ContentLine::new("DTSTART", utc_stamp(start)),
        ContentLine::new("DTEND", utc_stamp(end)),
        ContentLine::new("ORGANIZER", mailto(&query.organizer)),
    ] {
        out.push_str(&line.to_text());
    }
    for attendee in &query.attendees {
        out.push_str(&ContentLine::new("ATTENDEE", mailto(attendee)).to_text());
    }
    out.push_str(&ContentLine::new("END", "VFREEBUSY").to_text());
    out.push_str(&ContentLine::new("END", "VCALENDAR").to_text());
    Some(out)
}

/// What a free/busy reply says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeBusyReply<'a> {
    /// `UID` of the request answered.
    pub uid: &'a str,
    /// The asker.
    pub organizer: &'a str,
    /// Whose free/busy this is.
    pub attendee: &'a str,
    /// Range start (RFC 3339 UTC).
    pub from: &'a str,
    /// Range end (RFC 3339 UTC).
    pub to: &'a str,
    /// Busy periods (RFC 3339 UTC).
    pub periods: &'a [BusyPeriod],
}

/// A `VFREEBUSY` reply (`METHOD:REPLY`): one `FREEBUSY` line per period
/// (merged first), for answering an iMIP free/busy request or for the
/// mock server. `None` when the range is not RFC 3339 UTC.
pub fn build_reply(reply: &FreeBusyReply<'_>, dtstamp_unix: i64) -> Option<String> {
    let start = utc_seconds(reply.from)?;
    let end = utc_seconds(reply.to)?;
    let mut out = String::new();
    for line in [
        ContentLine::new("BEGIN", "VCALENDAR"),
        ContentLine::new("VERSION", "2.0"),
        ContentLine::new("PRODID", PRODID),
        ContentLine::new("METHOD", "REPLY"),
        ContentLine::new("BEGIN", "VFREEBUSY"),
        ContentLine::new("UID", reply.uid),
        ContentLine::new("DTSTAMP", utc_stamp(dtstamp_unix)),
        ContentLine::new("DTSTART", utc_stamp(start)),
        ContentLine::new("DTEND", utc_stamp(end)),
        ContentLine::new("ORGANIZER", mailto(reply.organizer)),
        ContentLine::new("ATTENDEE", mailto(reply.attendee)),
    ] {
        out.push_str(&line.to_text());
    }
    for period in BusyPeriod::merge(reply.periods.to_vec()) {
        let (Some(s), Some(e)) = (ical_stamp(&period.start), ical_stamp(&period.end)) else {
            continue;
        };
        let mut line = ContentLine::new("FREEBUSY", format!("{s}/{e}"));
        if period.kind != BusyKind::Busy {
            line.set_param("FBTYPE", fbtype(period.kind));
        }
        out.push_str(&line.to_text());
    }
    out.push_str(&ContentLine::new("END", "VFREEBUSY").to_text());
    out.push_str(&ContentLine::new("END", "VCALENDAR").to_text());
    Some(out)
}

// ---------------------------------------------------------------------------
// Parsing

/// One `VFREEBUSY` component.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VFreeBusy {
    /// `METHOD` of the calendar object (`REQUEST`, `REPLY`, `PUBLISH`).
    pub method: Option<String>,
    /// `UID`.
    pub uid: Option<String>,
    /// `ORGANIZER` address (without `mailto:`).
    pub organizer: Option<String>,
    /// `ATTENDEE` addresses (without `mailto:`).
    pub attendees: Vec<String>,
    /// `DTSTART` (RFC 3339 UTC).
    pub start: Option<String>,
    /// `DTEND` (RFC 3339 UTC).
    pub end: Option<String>,
    /// Busy periods (`FREE` left out), as written: not clipped or merged.
    pub periods: Vec<BusyPeriod>,
}

impl VFreeBusy {
    /// The periods clipped to `from`..`to` (RFC 3339 UTC) and merged.
    pub fn busy_in(&self, from: &str, to: &str) -> Vec<BusyPeriod> {
        clip(&self.periods, from, to)
    }
}

/// Clips periods to `from`..`to` (RFC 3339 UTC; unparsable bounds leave
/// that side open) and merges them.
pub fn clip(periods: &[BusyPeriod], from: &str, to: &str) -> Vec<BusyPeriod> {
    let lo = utc_seconds(from);
    let hi = utc_seconds(to);
    let mut out = Vec::with_capacity(periods.len());
    for p in periods {
        let (Some(mut s), Some(mut e)) = (utc_seconds(&p.start), utc_seconds(&p.end)) else {
            continue;
        };
        if let Some(lo) = lo {
            s = s.max(lo);
        }
        if let Some(hi) = hi {
            e = e.min(hi);
        }
        if e > s {
            out.push(BusyPeriod {
                start: rfc3339(s),
                end: rfc3339(e),
                kind: p.kind,
            });
        }
    }
    BusyPeriod::merge(out)
}

/// Parses one `FREEBUSY` value: comma-separated `start/end` or
/// `start/duration` periods.
pub fn parse_periods(value: &str, kind: BusyKind) -> Vec<BusyPeriod> {
    value
        .split(',')
        .filter_map(|period| {
            let (start, rest) = period.trim().split_once('/')?;
            let s = ical_seconds(start)?;
            let e = if rest.trim_start().starts_with(['P', 'p', '+']) {
                s.checked_add(duration_seconds(rest)?)?
            } else {
                ical_seconds(rest)?
            };
            (e > s).then(|| BusyPeriod {
                start: rfc3339(s),
                end: rfc3339(e),
                kind,
            })
        })
        .collect()
}

/// Every `VFREEBUSY` in an iCalendar text.
pub fn parse(text: &str) -> Vec<VFreeBusy> {
    let mut method = None;
    let mut out: Vec<VFreeBusy> = Vec::new();
    let mut current: Option<VFreeBusy> = None;
    let mut depth_in_fb = 0usize;
    for raw in crate::icalendar::logical_lines(text) {
        let Some(line) = ContentLine::parse(&raw.unfolded) else {
            continue;
        };
        match line.name.as_str() {
            "BEGIN" if line.value.trim().eq_ignore_ascii_case("VFREEBUSY") && current.is_none() => {
                current = Some(VFreeBusy::default());
                depth_in_fb = 0;
            }
            "BEGIN" if current.is_some() => depth_in_fb += 1,
            "END" if current.is_some() => {
                if depth_in_fb == 0 {
                    out.extend(current.take());
                } else {
                    depth_in_fb -= 1;
                }
            }
            "METHOD" if current.is_none() => {
                method = Some(line.value.trim().to_ascii_uppercase());
            }
            _ => {
                let Some(fb) = current.as_mut().filter(|_| depth_in_fb == 0) else {
                    continue;
                };
                match line.name.as_str() {
                    "UID" => fb.uid = Some(line.value.trim().to_owned()),
                    "ORGANIZER" => fb.organizer = Some(address_of(&line.value).to_owned()),
                    "ATTENDEE" => fb.attendees.push(address_of(&line.value).to_owned()),
                    "DTSTART" => fb.start = ical_seconds(&line.value).map(rfc3339),
                    "DTEND" => fb.end = ical_seconds(&line.value).map(rfc3339),
                    "FREEBUSY" => {
                        if let Some(kind) = busy_kind(line.param("FBTYPE")) {
                            let room = MAX_PERIODS.saturating_sub(fb.periods.len());
                            fb.periods
                                .extend(parse_periods(&line.value, kind).into_iter().take(room));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for fb in &mut out {
        fb.method.clone_from(&method);
    }
    out
}

// ---------------------------------------------------------------------------
// The user's own calendars

/// What counts when computing the user's own free/busy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnFreeBusy {
    /// Range start (RFC 3339 UTC).
    pub from: String,
    /// Range end (RFC 3339 UTC).
    pub to: String,
    /// Leave out this event (the one being edited, so it does not block
    /// itself).
    pub exclude_event: Option<EventId>,
    /// Only these calendars (`None` = every calendar in the instances;
    /// the caller leaves out calendars that do not count for the
    /// identity, e.g. subscriptions or holidays).
    pub calendars: Option<Vec<CalendarId>>,
}

/// The busy kind an instance contributes, `None` when it does not block
/// time: cancelled, transparent (`TRANSP:TRANSPARENT`), or the user
/// declined or delegated it. A tentative event, or one the user has not
/// answered or answered tentatively, is `BUSY-TENTATIVE`.
pub fn instance_kind(
    instance: &EventInstance,
    my_status: Option<ParticipationStatus>,
) -> Option<BusyKind> {
    if instance.status == EventStatus::Cancelled || instance.transparency == Transparency::Free {
        return None;
    }
    match my_status {
        Some(ParticipationStatus::Declined | ParticipationStatus::Delegated) => None,
        Some(ParticipationStatus::Tentative | ParticipationStatus::NeedsAction) => {
            Some(BusyKind::Tentative)
        }
        Some(ParticipationStatus::Accepted) | None => Some(match instance.status {
            EventStatus::Tentative => BusyKind::Tentative,
            _ => BusyKind::Busy,
        }),
    }
}

/// Busy periods of the user's own calendars from expanded instances
/// (`ExpandEvents` / the store's instance cache, already resolved to UTC):
/// filtered by [`instance_kind`], clipped to the range and merged.
/// `my_status` answers the user's `PARTSTAT` for meetings (`None` when the
/// user is the organizer or the event has no attendees).
pub fn own_busy_periods(
    instances: &[EventInstance],
    query: &OwnFreeBusy,
    my_status: impl Fn(&EventInstance) -> Option<ParticipationStatus>,
) -> Vec<BusyPeriod> {
    let periods: Vec<BusyPeriod> = instances
        .iter()
        .filter(|i| query.exclude_event.as_ref() != Some(&i.event_id))
        .filter(|i| {
            query
                .calendars
                .as_ref()
                .is_none_or(|c| c.contains(&i.calendar_id))
        })
        .filter_map(|i| {
            let kind = instance_kind(i, my_status(i))?;
            Some(BusyPeriod {
                start: i.start_utc.clone(),
                end: i.end_utc.clone(),
                kind,
            })
        })
        .collect();
    clip(&periods, &query.from, &query.to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::PimTime;
    use pretty_assertions::assert_eq;

    fn p(start: &str, end: &str, kind: BusyKind) -> BusyPeriod {
        BusyPeriod {
            start: start.to_owned(),
            end: end.to_owned(),
            kind,
        }
    }

    #[test]
    fn times() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        let s = utc_seconds("2026-10-01T09:30:15Z").unwrap();
        assert_eq!(rfc3339(s), "2026-10-01T09:30:15Z");
        assert_eq!(ical_seconds("20261001T093015Z"), Some(s));
        assert_eq!(ical_seconds("20261001T093015"), Some(s));
        assert_eq!(
            ical_seconds("20261001"),
            utc_seconds("2026-10-01T00:00:00Z")
        );
        for bad in [
            "",
            "2026",
            "20261001X093015Z",
            "2026100aT093015Z",
            "20261301T000000Z",
        ] {
            assert_eq!(ical_seconds(bad), None, "{bad}");
        }
        assert_eq!(
            ical_stamp("2026-10-01T09:30:15.250Z").as_deref(),
            Some("20261001T093015Z")
        );
        assert_eq!(ical_stamp("yesterday"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(duration_seconds("PT1H"), Some(3600));
        assert_eq!(duration_seconds("+PT30M"), Some(1800));
        assert_eq!(
            duration_seconds("P1DT2H3M4S"),
            Some(86_400 + 7200 + 180 + 4)
        );
        assert_eq!(duration_seconds("P2W"), Some(14 * 86_400));
        assert_eq!(duration_seconds("p1d"), Some(86_400));
        for bad in ["", "P", "PT", "-PT1H", "PT1", "P1H", "PT1D", "1H", "PT1H2"] {
            assert_eq!(duration_seconds(bad), None, "{bad}");
        }
    }

    #[test]
    fn fbtypes() {
        assert_eq!(busy_kind(None), Some(BusyKind::Busy));
        assert_eq!(busy_kind(Some("free")), None);
        assert_eq!(busy_kind(Some("BUSY-TENTATIVE")), Some(BusyKind::Tentative));
        assert_eq!(
            busy_kind(Some("busy-unavailable")),
            Some(BusyKind::Unavailable)
        );
        assert_eq!(busy_kind(Some("X-OOF")), Some(BusyKind::Busy));
        for kind in [BusyKind::Busy, BusyKind::Tentative, BusyKind::Unavailable] {
            assert_eq!(busy_kind(Some(fbtype(kind))), Some(kind));
        }
    }

    fn query() -> FreeBusyQuery {
        FreeBusyQuery {
            organizer: "ada@example.org".into(),
            attendees: vec!["bob@example.org".into(), "mailto:carol@example.org".into()],
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-02T00:00:00Z".into(),
        }
    }

    #[test]
    fn builds_requests() {
        let text = build_request(&query(), "fb-1", 0).unwrap();
        assert!(text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        for line in [
            "METHOD:REQUEST\r\n",
            "BEGIN:VFREEBUSY\r\n",
            "UID:fb-1\r\n",
            "DTSTAMP:19700101T000000Z\r\n",
            "DTSTART:20261001T000000Z\r\n",
            "DTEND:20261002T000000Z\r\n",
            "ORGANIZER:mailto:ada@example.org\r\n",
            "ATTENDEE:mailto:bob@example.org\r\n",
            "ATTENDEE:mailto:carol@example.org\r\n",
        ] {
            assert!(text.contains(line), "{line:?} in {text}");
        }
        let parsed = parse(&text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].method.as_deref(), Some("REQUEST"));
        assert_eq!(
            parsed[0].attendees,
            ["bob@example.org", "carol@example.org"]
        );
        assert_eq!(parsed[0].start.as_deref(), Some("2026-10-01T00:00:00Z"));
        let mut empty = query();
        empty.to.clone_from(&empty.from);
        assert_eq!(build_request(&empty, "x", 0), None);
        empty.to = "tomorrow".into();
        assert_eq!(build_request(&empty, "x", 0), None);
    }

    #[test]
    fn replies_round_trip() {
        let periods = [
            p(
                "2026-10-01T09:00:00Z",
                "2026-10-01T10:00:00Z",
                BusyKind::Busy,
            ),
            p(
                "2026-10-01T09:30:00Z",
                "2026-10-01T11:00:00Z",
                BusyKind::Busy,
            ),
            p(
                "2026-10-01T13:00:00Z",
                "2026-10-01T14:00:00Z",
                BusyKind::Tentative,
            ),
            p(
                "2026-10-01T15:00:00Z",
                "2026-10-01T16:00:00Z",
                BusyKind::Unavailable,
            ),
        ];
        let text = build_reply(
            &FreeBusyReply {
                uid: "fb-1",
                organizer: "ada@example.org",
                attendee: "bob@example.org",
                from: "2026-10-01T00:00:00Z",
                to: "2026-10-02T00:00:00Z",
                periods: &periods,
            },
            0,
        )
        .unwrap();
        assert!(text.contains("METHOD:REPLY\r\n"));
        assert!(text.contains("FREEBUSY:20261001T090000Z/20261001T110000Z\r\n"));
        assert!(
            text.contains("FREEBUSY;FBTYPE=BUSY-TENTATIVE:20261001T130000Z/20261001T140000Z\r\n")
        );
        let parsed = parse(&text);
        assert_eq!(parsed[0].method.as_deref(), Some("REPLY"));
        assert_eq!(parsed[0].organizer.as_deref(), Some("ada@example.org"));
        assert_eq!(parsed[0].attendees, ["bob@example.org"]);
        assert_eq!(
            parsed[0].busy_in("2026-10-01T00:00:00Z", "2026-10-02T00:00:00Z"),
            vec![
                p(
                    "2026-10-01T09:00:00Z",
                    "2026-10-01T11:00:00Z",
                    BusyKind::Busy
                ),
                p(
                    "2026-10-01T13:00:00Z",
                    "2026-10-01T14:00:00Z",
                    BusyKind::Tentative
                ),
                p(
                    "2026-10-01T15:00:00Z",
                    "2026-10-01T16:00:00Z",
                    BusyKind::Unavailable
                ),
            ]
        );
    }

    #[test]
    fn parses_real_world_shapes() {
        // Folded lines, several periods per line, durations, FREE periods,
        // lower-case names, a nested component that is ignored, and a
        // second VFREEBUSY.
        let text = "BEGIN:VCALENDAR\nmethod:reply\nBEGIN:VFREEBUSY\nUID:x\nATTENDEE;CN=Bob:MAILTO:Bob@Example.org\n\
                    FREEBUSY;FBTYPE=BUSY:20261001T080000Z/PT1H,20261001T120000Z/20261001T1\n 30000Z\n\
                    freebusy;fbtype=FREE:20261001T140000Z/PT8H\n\
                    FREEBUSY:20261001T230000Z/PT2H,garbage,20261001T100000Z/20261001T090000Z\n\
                    BEGIN:X-NESTED\nFREEBUSY:20261001T000000Z/PT1H\nEND:X-NESTED\n\
                    END:VFREEBUSY\nBEGIN:VFREEBUSY\nATTENDEE:mailto:carol@example.org\nEND:VFREEBUSY\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].method.as_deref(), Some("REPLY"));
        assert_eq!(parsed[0].attendees, ["Bob@Example.org"]);
        assert_eq!(parsed[0].periods.len(), 3, "{:?}", parsed[0].periods);
        assert_eq!(
            parsed[0].busy_in("2026-10-01T00:00:00Z", "2026-10-02T00:00:00Z"),
            vec![
                p(
                    "2026-10-01T08:00:00Z",
                    "2026-10-01T09:00:00Z",
                    BusyKind::Busy
                ),
                p(
                    "2026-10-01T12:00:00Z",
                    "2026-10-01T13:00:00Z",
                    BusyKind::Busy
                ),
                p(
                    "2026-10-01T23:00:00Z",
                    "2026-10-02T00:00:00Z",
                    BusyKind::Busy
                ),
            ],
            "clipped at the range end"
        );
        assert_eq!(parsed[1].attendees, ["carol@example.org"]);
        assert!(parsed[1].periods.is_empty());
        assert!(parse("not a calendar").is_empty());
    }

    #[test]
    fn caps_periods() {
        let many: Vec<String> = (0..(MAX_PERIODS + 10))
            .map(|i| {
                let s = 1_800_000_000 + i64::try_from(i).unwrap() * 7200;
                format!("{}/PT1H", utc_stamp(s))
            })
            .collect();
        let text = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VFREEBUSY\r\nFREEBUSY:{}\r\nFREEBUSY:{}\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n",
            many.join(","),
            many[0]
        );
        assert_eq!(parse(&text)[0].periods.len(), MAX_PERIODS);
    }

    fn instance(id: &str, start: &str, end: &str) -> EventInstance {
        EventInstance {
            event_id: EventId::new(id),
            calendar_id: CalendarId::new("cal-1"),
            recurrence_id: None,
            start: PimTime::zoned(start.trim_end_matches('Z'), "UTC"),
            end: PimTime::zoned(end.trim_end_matches('Z'), "UTC"),
            start_utc: start.to_owned(),
            end_utc: end.to_owned(),
            all_day: false,
            title: id.to_owned(),
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

    #[test]
    fn own_calendars() {
        let mut tentative = instance("tentative", "2026-10-01T11:00:00Z", "2026-10-01T12:00:00Z");
        tentative.status = EventStatus::Tentative;
        let mut free = instance("free", "2026-10-01T06:00:00Z", "2026-10-01T07:00:00Z");
        free.transparency = Transparency::Free;
        let mut cancelled = instance("cancelled", "2026-10-01T06:00:00Z", "2026-10-01T07:00:00Z");
        cancelled.status = EventStatus::Cancelled;
        let mut other_calendar =
            instance("holiday", "2026-10-01T00:00:00Z", "2026-10-02T00:00:00Z");
        other_calendar.calendar_id = CalendarId::new("holidays");
        let instances = vec![
            instance("a", "2026-10-01T09:00:00Z", "2026-10-01T10:00:00Z"),
            instance("b", "2026-10-01T09:30:00.500Z", "2026-10-01T10:30:00Z"),
            tentative,
            free,
            cancelled,
            instance("declined", "2026-10-01T15:00:00Z", "2026-10-01T16:00:00Z"),
            instance("unanswered", "2026-10-01T17:00:00Z", "2026-10-01T18:00:00Z"),
            instance("delegated", "2026-10-01T19:00:00Z", "2026-10-01T20:00:00Z"),
            instance("editing", "2026-10-01T13:00:00Z", "2026-10-01T14:00:00Z"),
            instance("early", "2026-09-30T23:00:00Z", "2026-10-01T01:00:00Z"),
            instance("outside", "2026-10-03T09:00:00Z", "2026-10-03T10:00:00Z"),
            other_calendar,
        ];
        let query = OwnFreeBusy {
            from: "2026-10-01T00:00:00Z".into(),
            to: "2026-10-02T00:00:00Z".into(),
            exclude_event: Some(EventId::new("editing")),
            calendars: Some(vec![CalendarId::new("cal-1")]),
        };
        let status = |i: &EventInstance| match i.event_id.as_str() {
            "declined" => Some(ParticipationStatus::Declined),
            "unanswered" => Some(ParticipationStatus::NeedsAction),
            "delegated" => Some(ParticipationStatus::Delegated),
            "a" => Some(ParticipationStatus::Accepted),
            _ => None,
        };
        assert_eq!(
            own_busy_periods(&instances, &query, status),
            vec![
                p(
                    "2026-10-01T00:00:00Z",
                    "2026-10-01T01:00:00Z",
                    BusyKind::Busy
                ),
                p(
                    "2026-10-01T09:00:00Z",
                    "2026-10-01T10:30:00Z",
                    BusyKind::Busy
                ),
                p(
                    "2026-10-01T11:00:00Z",
                    "2026-10-01T12:00:00Z",
                    BusyKind::Tentative
                ),
                p(
                    "2026-10-01T17:00:00Z",
                    "2026-10-01T18:00:00Z",
                    BusyKind::Tentative
                ),
            ]
        );
        let all = OwnFreeBusy {
            exclude_event: None,
            calendars: None,
            ..query
        };
        let periods = own_busy_periods(&instances, &all, |_| None);
        assert!(
            periods
                .iter()
                .any(|p| p.start == "2026-10-01T00:00:00Z" && p.end == "2026-10-02T00:00:00Z")
        );
        assert!(own_busy_periods(&[], &all, |_| None).is_empty());
    }
}
