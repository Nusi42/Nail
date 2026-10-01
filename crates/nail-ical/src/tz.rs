//! Time zones: `TZID`s → IANA zones (`chrono-tz`).
//!
//! Resolution order for a `TZID` (CalConnect's advice: trust names over
//! definitions when the name is known):
//!
//! 1. an IANA name (`Europe/Berlin`), also with the `/mozilla.org/…` or
//!    `/softwarestudio.org/…/` prefixes of old Thunderbird / libical
//!    exports;
//! 2. Windows / Outlook / Exchange names (`W. Europe Standard Time`,
//!    `(UTC+01:00) Amsterdam, Berlin, …`) through calcard's table;
//! 3. the `VTIMEZONE`'s `X-LIC-LOCATION` or `X-MICROSOFT-CDO-TZID`;
//! 4. matching the `VTIMEZONE`'s rules against every IANA zone around the
//!    item's dates (same UTC offsets every day for three years), preferring
//!    a zone whose city appears in the `TZID`, then common zones;
//! 5. a fixed-offset `VTIMEZONE` becomes `Etc/GMT±N`.
//!
//! What still has no IANA zone keeps its own definition
//! ([`VTimezone::to_utc`]) and is converted to UTC by the reader.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone};
use chrono_tz::Tz;
use nail_api::pim::PimTime;
use nail_api::pim::calendar::{Frequency, RecurrenceEnd, RecurrenceRule};

use crate::content::Component;
use crate::recur::{self, RuleIter, UntilBound};
use crate::time::{self, IcalValue};

/// Zones preferred when several IANA zones match a custom definition.
const PREFERRED: &[&str] = &[
    "Europe/London",
    "Europe/Berlin",
    "Europe/Paris",
    "Europe/Helsinki",
    "Europe/Athens",
    "Europe/Moscow",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Phoenix",
    "America/Los_Angeles",
    "America/Anchorage",
    "Pacific/Honolulu",
    "America/Halifax",
    "America/Sao_Paulo",
    "Asia/Tokyo",
    "Asia/Shanghai",
    "Asia/Kolkata",
    "Asia/Dubai",
    "Asia/Singapore",
    "Australia/Sydney",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Perth",
    "Pacific/Auckland",
    "Africa/Johannesburg",
];

/// Legacy alias prefixes skipped when choosing among matches.
const LEGACY_PREFIXES: &[&str] = &[
    "Etc/",
    "US/",
    "Canada/",
    "Brazil/",
    "Mexico/",
    "Chile/",
    "SystemV/",
    "Antarctica/",
];

/// The IANA zone for a zone name: IANA names (with or without a
/// `/vendor/…/` prefix), `UTC`, Windows / Outlook names and fixed offsets
/// (`Etc/GMT±N`).
pub fn iana_for_name(name: &str) -> Option<Tz> {
    let name = name.trim().trim_matches('"');
    if name.is_empty() {
        return None;
    }
    if let Some(tz) = time::zone(name) {
        return Some(tz);
    }
    // `/mozilla.org/20050126_1/Europe/Berlin`, `/citadel.org/…/Europe/Berlin`.
    if name.starts_with('/') {
        let segments: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).collect();
        for skip in 0..segments.len() {
            let candidate = segments[skip..].join("/");
            let candidate = candidate.strip_prefix("SystemV/").unwrap_or(&candidate);
            if let Some(tz) = time::zone(candidate) {
                return Some(tz);
            }
        }
    }
    match calcard::common::timezone::Tz::from_str(name).ok()? {
        calcard::common::timezone::Tz::Tz(tz) => Some(tz),
        calcard::common::timezone::Tz::Fixed(offset) => fixed_offset_zone(offset.local_minus_utc()),
        calcard::common::timezone::Tz::Floating => None,
    }
}

/// `Etc/GMT-1` for UTC+1 (POSIX signs are inverted); whole hours only.
pub fn fixed_offset_zone(offset_secs: i32) -> Option<Tz> {
    if offset_secs == 0 {
        return Some(Tz::UTC);
    }
    if offset_secs % 3600 != 0 {
        return None;
    }
    let hours = -offset_secs / 3600;
    let name = if hours > 0 {
        format!("Etc/GMT+{hours}")
    } else {
        format!("Etc/GMT{hours}")
    };
    name.parse().ok()
}

fn parse_offset(text: &str) -> Option<i32> {
    let text = text.trim();
    let (sign, rest) = match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if !(digits.len() == 4 || digits.len() == 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i32 = digits[..2].parse().ok()?;
    let m: i32 = digits[2..4].parse().ok()?;
    let s: i32 = if digits.len() == 6 {
        digits[4..].parse().ok()?
    } else {
        0
    };
    Some(sign * (h * 3600 + m * 60 + s))
}

#[derive(Debug, Clone)]
struct Observance {
    start: NaiveDateTime,
    offset_from: i32,
    offset_to: i32,
    rule: Option<RecurrenceRule>,
    dates: Vec<NaiveDateTime>,
}

/// One offset change: from this UTC instant on, `offset` applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Transition {
    utc: NaiveDateTime,
    before: i32,
    offset: i32,
}

/// A parsed `VTIMEZONE` (its `STANDARD` / `DAYLIGHT` observances).
#[derive(Debug, Clone)]
pub struct VTimezone {
    /// The `TZID`.
    pub tzid: String,
    lic_location: Option<String>,
    cdo_id: Option<String>,
    observances: Vec<Observance>,
}

impl VTimezone {
    /// Reads a `VTIMEZONE` component; `None` without a `TZID`.
    pub fn parse(component: &Component) -> Option<Self> {
        let tzid = component.text("TZID")?;
        let mut observances = Vec::new();
        for sub in &component.components {
            if !(sub.is("STANDARD") || sub.is("DAYLIGHT")) {
                continue;
            }
            let Some(start) = sub
                .property("DTSTART")
                .and_then(|p| time::parse_ical_value(&p.value))
                .map(|v| match v {
                    IcalValue::Date(d) => d.and_time(NaiveTime::MIN),
                    IcalValue::DateTime { local, .. } => local,
                })
            else {
                continue;
            };
            let (Some(offset_from), Some(offset_to)) = (
                sub.property("TZOFFSETFROM")
                    .and_then(|p| parse_offset(&p.value)),
                sub.property("TZOFFSETTO")
                    .and_then(|p| parse_offset(&p.value)),
            ) else {
                continue;
            };
            let floating = PimTime::floating(time::format_local(start));
            let rule = sub
                .property("RRULE")
                .and_then(|p| recur::parse_rrule(&p.value, &floating));
            let dates = sub
                .properties_named("RDATE")
                .flat_map(|p| p.value.split(',').map(str::to_owned).collect::<Vec<_>>())
                .filter_map(|v| match time::parse_ical_value(&v)? {
                    IcalValue::Date(d) => Some(d.and_time(NaiveTime::MIN)),
                    IcalValue::DateTime { local, .. } => Some(local),
                })
                .collect();
            observances.push(Observance {
                start,
                offset_from,
                offset_to,
                rule,
                dates,
            });
        }
        Some(Self {
            tzid,
            lic_location: component.text("X-LIC-LOCATION"),
            cdo_id: component.text("X-MICROSOFT-CDO-TZID"),
            observances,
        })
    }

    /// Offset changes up to the end of `last_year`, oldest first.
    fn transitions(&self, first_year: i32, last_year: i32) -> Vec<Transition> {
        let end = NaiveDate::from_ymd_opt(last_year + 1, 1, 1)
            .unwrap_or(NaiveDate::MAX)
            .and_time(NaiveTime::MIN);
        let mut out = Vec::new();
        for observance in &self.observances {
            let mut onsets = vec![observance.start];
            onsets.extend(observance.dates.iter().copied());
            if let Some(rule) = &observance.rule {
                // Yearly rules without COUNT are fast-forwarded close to
                // the window (VTIMEZONEs often start in 1601).
                let mut start = observance.start;
                if rule.freq == Frequency::Yearly
                    && !matches!(rule.end, RecurrenceEnd::Count { .. })
                    && start.year() < first_year - 1
                    && let Some(moved) = start.with_year(first_year - 1)
                {
                    start = moved;
                }
                let until = match &rule.end {
                    RecurrenceEnd::Until { until } => time::wall(until).map(|u| {
                        // UNTIL is UTC; onsets are local in `offset_from`.
                        let local = if until.tz().is_some() {
                            u + Duration::seconds(i64::from(observance.offset_from))
                        } else {
                            u
                        };
                        UntilBound::Wall(local)
                    }),
                    _ => None,
                };
                for onset in RuleIter::new(rule, start, false, until) {
                    if onset >= end {
                        break;
                    }
                    onsets.push(onset);
                }
            }
            for onset in onsets {
                if onset < end {
                    out.push(Transition {
                        utc: onset - Duration::seconds(i64::from(observance.offset_from)),
                        before: observance.offset_from,
                        offset: observance.offset_to,
                    });
                }
            }
        }
        out.sort_by_key(|t| t.utc);
        out.dedup_by_key(|t| t.utc);
        out
    }

    fn offset_in(transitions: &[Transition], utc: NaiveDateTime) -> Option<i32> {
        let index = transitions.partition_point(|t| t.utc <= utc);
        match index {
            0 => transitions.first().map(|t| t.before),
            i => Some(transitions[i - 1].offset),
        }
    }

    /// The UTC offset (seconds east) in force at a UTC time.
    pub fn offset_at(&self, utc: NaiveDateTime) -> Option<i32> {
        let transitions = self.transitions(utc.year() - 1, utc.year() + 1);
        Self::offset_in(&transitions, utc)
    }

    /// The UTC time of a wall-clock time in this zone (gaps shift forward,
    /// folds take the earlier instant). `None` without observances.
    pub fn to_utc(&self, local: NaiveDateTime) -> Option<NaiveDateTime> {
        let transitions = self.transitions(local.year() - 1, local.year() + 1);
        let mut offsets: Vec<i32> = transitions
            .iter()
            .flat_map(|t| [t.before, t.offset])
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        // Larger offsets give earlier instants: the first match wins.
        for &offset in offsets.iter().rev() {
            let utc = local - Duration::seconds(i64::from(offset));
            if Self::offset_in(&transitions, utc) == Some(offset) {
                return Some(utc);
            }
        }
        // A gap: the offset before it applies.
        let before = Self::offset_in(&transitions, local - Duration::hours(30))?;
        Some(local - Duration::seconds(i64::from(before)))
    }

    /// The IANA zone this definition stands for, judged around `year`.
    pub fn iana(&self, year: i32) -> Option<Tz> {
        if let Some(tz) = iana_for_name(&self.tzid) {
            return Some(tz);
        }
        if let Some(tz) = self.lic_location.as_deref().and_then(iana_for_name) {
            return Some(tz);
        }
        if let Some(tz) = self
            .cdo_id
            .as_deref()
            .and_then(calcard::common::timezone::Tz::from_ms_cdo_zone_id)
            .and_then(|tz| match tz {
                calcard::common::timezone::Tz::Tz(tz) => Some(tz),
                _ => None,
            })
        {
            return Some(tz);
        }
        self.matching_zone(year)
    }

    fn matching_zone(&self, year: i32) -> Option<Tz> {
        let transitions = self.transitions(year - 1, year + 1);
        if transitions.is_empty() {
            return None;
        }
        let first = NaiveDate::from_ymd_opt(year - 1, 1, 1)?.and_hms_opt(12, 0, 0)?;
        let samples: Vec<(NaiveDateTime, i32)> = (0..(3 * 365))
            .filter_map(|day| {
                let at = first + Duration::days(day);
                Some((at, Self::offset_in(&transitions, at)?))
            })
            .collect();
        let quick: Vec<(NaiveDateTime, i32)> = samples.iter().step_by(91).copied().collect();
        let offset_of =
            |tz: Tz, at: NaiveDateTime| tz.offset_from_utc_datetime(&at).fix().local_minus_utc();
        let matches: Vec<Tz> = chrono_tz::TZ_VARIANTS
            .iter()
            .copied()
            .filter(|tz| quick.iter().all(|&(at, o)| offset_of(*tz, at) == o))
            .filter(|tz| samples.iter().all(|&(at, o)| offset_of(*tz, at) == o))
            .collect();
        if matches.is_empty() {
            let offsets: Vec<i32> = transitions
                .iter()
                .flat_map(|t| [t.before, t.offset])
                .collect();
            return match offsets.first() {
                Some(&o) if offsets.iter().all(|&x| x == o) => fixed_offset_zone(o),
                _ => None,
            };
        }
        let tzid = self.tzid.to_ascii_lowercase().replace(' ', "_");
        let city = |tz: &Tz| {
            tz.name()
                .rsplit('/')
                .next()
                .map(str::to_ascii_lowercase)
                .unwrap_or_default()
        };
        if let Some(tz) = matches.iter().find(|tz| {
            let city = city(tz);
            city.len() > 2 && tzid.contains(&city)
        }) {
            return Some(*tz);
        }
        // A definition without DST is a fixed offset, not a city.
        let first_offset = transitions[0].before;
        if transitions
            .iter()
            .all(|t| t.before == first_offset && t.offset == first_offset)
            && let Some(fixed) = fixed_offset_zone(first_offset)
        {
            return Some(fixed);
        }
        for name in PREFERRED {
            if let Some(tz) = matches.iter().find(|tz| tz.name() == *name) {
                return Some(*tz);
            }
        }
        matches
            .iter()
            .find(|tz| {
                tz.name().contains('/') && !LEGACY_PREFIXES.iter().any(|p| tz.name().starts_with(p))
            })
            .or(matches.first())
            .copied()
    }
}

/// How a `TZID` resolved.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// An IANA zone.
    Iana(Tz),
    /// Only the item's own definition (times are converted to UTC).
    Custom(VTimezone),
    /// Neither a known name nor a definition (times are read as floating).
    Unknown,
}

/// Resolves the `TZID`s of one calendar object, with its `VTIMEZONE`s.
#[derive(Debug, Clone, Default)]
pub struct ZoneResolver {
    definitions: HashMap<String, VTimezone>,
    cache: std::cell::RefCell<HashMap<String, Resolution>>,
    year: i32,
}

impl ZoneResolver {
    /// A resolver over the `VTIMEZONE`s of `calendar`; custom definitions
    /// are matched around `year` (the calendar's dates).
    pub fn new(calendar: &Component, year: i32) -> Self {
        let definitions = calendar
            .components_named("VTIMEZONE")
            .filter_map(VTimezone::parse)
            .map(|tz| (tz.tzid.clone(), tz))
            .collect();
        Self {
            definitions,
            cache: Default::default(),
            year,
        }
    }

    /// The `VTIMEZONE` defined for a `TZID`, if any.
    pub fn definition(&self, tzid: &str) -> Option<&VTimezone> {
        self.definitions.get(tzid)
    }

    /// Resolves a `TZID`.
    pub fn resolve(&self, tzid: &str) -> Resolution {
        if let Some(hit) = self.cache.borrow().get(tzid) {
            return hit.clone();
        }
        let resolution = match iana_for_name(tzid) {
            Some(tz) => Resolution::Iana(tz),
            None => match self.definitions.get(tzid) {
                Some(definition) => match definition.iana(self.year) {
                    Some(tz) => Resolution::Iana(tz),
                    None => Resolution::Custom(definition.clone()),
                },
                None => Resolution::Unknown,
            },
        };
        self.cache
            .borrow_mut()
            .insert(tzid.to_owned(), resolution.clone());
        resolution
    }

    /// The original `TZID` used in this object for an IANA zone, so a
    /// rewritten time keeps the producer's spelling (`W. Europe Standard
    /// Time`).
    pub fn original_tzid(&self, iana: &str) -> Option<String> {
        let mut names: Vec<&String> = self.definitions.keys().collect();
        names.sort();
        names
            .into_iter()
            .find(|name| matches!(self.resolve(name), Resolution::Iana(tz) if tz.name() == iana))
            .cloned()
    }
}

/// A `VTIMEZONE` for an IANA zone, generated from `chrono-tz` for the
/// years `from_year..=to_year` (calcard's generator: regular rules become
/// `RRULE`s), parsed back into Nail's content model.
pub fn vtimezone_for(tz: Tz, from_year: i32, to_year: i32) -> Option<Component> {
    if tz == Tz::UTC {
        return None;
    }
    let mut calendar =
        calcard::icalendar::ICalendar::parse("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n")
            .ok()?;
    let from = NaiveDate::from_ymd_opt(from_year, 1, 1)?
        .and_time(NaiveTime::MIN)
        .and_utc()
        .timestamp();
    let to = NaiveDate::from_ymd_opt(to_year + 1, 1, 1)?
        .and_time(NaiveTime::MIN)
        .and_utc()
        .timestamp();
    calendar.add_timezone(tz.name(), from, to)?;
    let text = calendar.to_string();
    let doc = crate::content::parse(&text);
    let mut zone = doc
        .components
        .into_iter()
        .next()?
        .components
        .into_iter()
        .find(|c| c.is("VTIMEZONE"))?;
    for property in &mut zone.properties {
        property.touch();
    }
    Some(zone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse;

    const OUTLOOK_BERLIN: &str = "BEGIN:VCALENDAR\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Mitteleuropäische Zeit\r\n\
BEGIN:STANDARD\r\n\
DTSTART:16011028T030000\r\n\
RRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\n\
TZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
DTSTART:16010325T020000\r\n\
RRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\n\
TZOFFSETFROM:+0100\r\n\
TZOFFSETTO:+0200\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Custom Mars Time\r\n\
BEGIN:STANDARD\r\n\
DTSTART:19700101T000000\r\n\
TZOFFSETFROM:+0537\r\n\
TZOFFSETTO:+0537\r\n\
END:STANDARD\r\n\
END:VTIMEZONE\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Fixed Plus Three\r\n\
BEGIN:STANDARD\r\n\
DTSTART:19700101T000000\r\n\
TZOFFSETFROM:+0300\r\n\
TZOFFSETTO:+0300\r\n\
END:STANDARD\r\n\
END:VTIMEZONE\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Customized Time Zone\r\n\
X-MICROSOFT-CDO-TZID:4\r\n\
END:VTIMEZONE\r\n\
END:VCALENDAR\r\n";

    fn at(text: &str) -> NaiveDateTime {
        time::parse_local(text).unwrap()
    }

    #[test]
    fn names_resolve_to_iana() {
        assert_eq!(
            iana_for_name("Europe/Berlin"),
            Some(chrono_tz::Europe::Berlin)
        );
        assert_eq!(iana_for_name("UTC"), Some(Tz::UTC));
        assert_eq!(
            iana_for_name("W. Europe Standard Time"),
            Some(chrono_tz::Europe::Berlin)
        );
        assert_eq!(
            iana_for_name("Eastern Standard Time"),
            Some(chrono_tz::America::New_York)
        );
        assert_eq!(
            iana_for_name("Pacific Standard Time"),
            Some(chrono_tz::America::Los_Angeles)
        );
        assert_eq!(
            iana_for_name("(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna"),
            Some(chrono_tz::Europe::Berlin)
        );
        assert_eq!(
            iana_for_name("/mozilla.org/20050126_1/Europe/Berlin"),
            Some(chrono_tz::Europe::Berlin)
        );
        assert_eq!(
            iana_for_name("/softwarestudio.org/Olson_20011030_5/America/Chicago"),
            Some(chrono_tz::America::Chicago)
        );
        assert_eq!(iana_for_name("Nowhere Standard Time"), None);
        assert_eq!(fixed_offset_zone(3600).map(|t| t.name()), Some("Etc/GMT-1"));
        assert_eq!(
            fixed_offset_zone(-5 * 3600).map(|t| t.name()),
            Some("Etc/GMT+5")
        );
        assert_eq!(fixed_offset_zone(1800), None);
    }

    #[test]
    fn vtimezones_resolve_by_rules_cdo_ids_and_offsets() {
        let doc = parse(OUTLOOK_BERLIN);
        let resolver = ZoneResolver::new(&doc.components[0], 2026);
        let berlin = match resolver.resolve("Mitteleuropäische Zeit") {
            Resolution::Iana(tz) => tz,
            other => panic!("{other:?}"),
        };
        // Same rules as Berlin; the preferred zone wins.
        assert_eq!(berlin.name(), "Europe/Berlin");
        assert!(matches!(
            resolver.resolve("Customized Time Zone"),
            Resolution::Iana(chrono_tz::Europe::Berlin)
        ));
        assert!(matches!(
            resolver.resolve("Fixed Plus Three"),
            Resolution::Iana(tz) if tz.name() == "Etc/GMT-3"
        ));
        let Resolution::Custom(mars) = resolver.resolve("Custom Mars Time") else {
            panic!("expected a custom zone");
        };
        assert_eq!(
            mars.to_utc(at("2026-10-01T12:00:00")),
            Some(at("2026-10-01T06:23:00"))
        );
        assert!(matches!(
            resolver.resolve("Unheard Of"),
            Resolution::Unknown
        ));
        assert_eq!(
            resolver.original_tzid("Europe/Berlin").as_deref(),
            Some("Customized Time Zone")
        );
    }

    #[test]
    fn custom_definitions_handle_gaps_and_folds() {
        let doc = parse(OUTLOOK_BERLIN);
        let tz = VTimezone::parse(&doc.components[0].components[0]).unwrap();
        assert_eq!(tz.offset_at(at("2026-07-01T00:00:00")), Some(7200));
        assert_eq!(tz.offset_at(at("2026-12-01T00:00:00")), Some(3600));
        // Gap: 02:30 on March 29 → 01:30Z (like chrono-tz Berlin).
        assert_eq!(
            tz.to_utc(at("2026-03-29T02:30:00")),
            Some(at("2026-03-29T01:30:00"))
        );
        // Fold: 02:30 on October 25 → the first, 00:30Z.
        assert_eq!(
            tz.to_utc(at("2026-10-25T02:30:00")),
            Some(at("2026-10-25T00:30:00"))
        );
        assert_eq!(
            tz.to_utc(at("2026-10-25T03:30:00")),
            Some(at("2026-10-25T02:30:00"))
        );
    }

    #[test]
    fn generated_vtimezones_round_trip_through_the_matcher() {
        let zone = vtimezone_for(chrono_tz::America::New_York, 2025, 2027).unwrap();
        let text = zone.to_text();
        assert!(text.contains("TZID:America/New_York"), "{text}");
        assert!(text.contains("BEGIN:DAYLIGHT"), "{text}");
        let mut renamed = parse(&text).components.remove(0);
        for p in &mut renamed.properties {
            if p.is("TZID") {
                p.set_value("Some Custom Name");
            }
        }
        let tz = VTimezone::parse(&renamed).unwrap();
        assert_eq!(tz.iana(2026), Some(chrono_tz::America::New_York));
        assert!(vtimezone_for(Tz::UTC, 2025, 2026).is_none());
    }

    #[test]
    fn offsets_parse() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0330"), Some(-12_600));
        assert_eq!(parse_offset("+013045"), Some(5445));
        assert_eq!(parse_offset("0100"), Some(3600));
        assert_eq!(parse_offset("+1"), None);
    }
}
