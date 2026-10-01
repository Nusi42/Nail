//! Just enough iCalendar for the mock server's scheduling (RFC 6638):
//! unfolding, content lines with parameters, UTC times and durations,
//! the events of an object, and line rewriting. Not a general parser:
//! zoned and floating times count as UTC, recurrences are not expanded.

/// One unfolded content line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Line {
    /// Upper-case name.
    pub name: String,
    /// Upper-case parameter names with unquoted values.
    pub params: Vec<(String, String)>,
    /// The value.
    pub value: String,
}

impl Line {
    pub fn new(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            params: Vec::new(),
            value: value.to_owned(),
        }
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_param(&mut self, name: &str, value: &str) {
        if let Some(slot) = self.params.iter_mut().find(|(n, _)| n == name) {
            value.clone_into(&mut slot.1);
        } else {
            self.params.push((name.to_owned(), value.to_owned()));
        }
    }

    pub fn remove_param(&mut self, name: &str) {
        self.params.retain(|(n, _)| n != name);
    }

    pub fn text(&self) -> String {
        let mut out = self.name.clone();
        for (name, value) in &self.params {
            out.push(';');
            out.push_str(name);
            out.push('=');
            if value.contains([';', ':', ',']) {
                out.push('"');
                out.push_str(value);
                out.push('"');
            } else {
                out.push_str(value);
            }
        }
        out.push(':');
        out.push_str(&self.value);
        out
    }
}

/// Logical lines (folds removed, CRLF or LF).
pub(super) fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for physical in text.split('\n') {
        let physical = physical.trim_end_matches('\r');
        if let Some(rest) = physical.strip_prefix([' ', '\t'])
            && let Some(last) = out.last_mut()
        {
            last.push_str(rest);
        } else if !physical.is_empty() {
            out.push(physical.to_owned());
        }
    }
    out
}

pub(super) fn parse_line(text: &str) -> Option<Line> {
    let mut quoted = false;
    let colon = text.char_indices().find_map(|(i, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(i),
        _ => None,
    })?;
    let head = &text[..colon];
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in head.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&head[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&head[start..]);
    let mut parts = parts.into_iter();
    let name = parts.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = parts
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (k.trim().to_ascii_uppercase(), v.replace('"', ""))
        })
        .collect();
    Some(Line {
        name,
        params,
        value: text[colon + 1..].to_owned(),
    })
}

/// The address of a calendar-user URI, lower-case, without `mailto:`.
pub(super) fn address(uri: &str) -> String {
    let uri = uri.trim();
    let bare = match uri.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("mailto:") => &uri[7..],
        _ => uri,
    };
    bare.trim().to_ascii_lowercase()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYYMMDD[THHMMSS[Z]]` (any zone counts as UTC) → Unix seconds.
pub(super) fn seconds(value: &str) -> Option<i64> {
    let v = value.trim().trim_end_matches(['Z', 'z']);
    let num = |r: std::ops::Range<usize>| v.get(r)?.parse::<i64>().ok();
    let days = days_from_civil(num(0..4)?, num(4..6)?, num(6..8)?);
    match v.len() {
        8 => Some(days * 86_400),
        15 if v.as_bytes()[8].eq_ignore_ascii_case(&b'T') => {
            Some(days * 86_400 + num(9..11)? * 3600 + num(11..13)? * 60 + num(13..15)?)
        }
        _ => None,
    }
}

/// Unix seconds → `YYYYMMDDTHHMMSSZ`.
pub(super) fn stamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A positive RFC 5545 duration in seconds (`P1W`, `P1DT2H`, `PT30M`).
pub(super) fn duration(value: &str) -> Option<i64> {
    let rest = value.trim().trim_start_matches('+').strip_prefix('P')?;
    let mut total = 0i64;
    let mut number = String::new();
    let mut time = false;
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' => time = true,
            unit => {
                let n: i64 = std::mem::take(&mut number).parse().ok()?;
                total += n * match (unit, time) {
                    ('W', false) => 604_800,
                    ('D', false) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    number.is_empty().then_some(total)
}

/// An attendee of an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attendee {
    pub email: String,
    pub partstat: String,
    /// `SCHEDULE-AGENT` is `SERVER` (or absent).
    pub server: bool,
}

/// The parts of a `VEVENT` the mock looks at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Event {
    pub uid: String,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub transparent: bool,
    pub status: String,
    pub organizer: Option<String>,
    /// `SCHEDULE-AGENT` of the organizer is `SERVER` (or absent).
    pub organizer_server: bool,
    pub attendees: Vec<Attendee>,
}

fn is_server_agent(line: &Line) -> bool {
    line.param("SCHEDULE-AGENT")
        .is_none_or(|a| !matches!(a.to_ascii_uppercase().as_str(), "CLIENT" | "NONE"))
}

/// The `VEVENT`s of an object (overrides included as separate events).
pub(super) fn events(text: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut current: Option<(Event, Option<i64>, Option<bool>)> = None;
    let mut nested = 0usize;
    for raw in unfold(text) {
        let Some(line) = parse_line(&raw) else {
            continue;
        };
        let value = line.value.trim();
        match line.name.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VEVENT") && current.is_none() => {
                current = Some((Event::default(), None, None));
                nested = 0;
            }
            "BEGIN" if current.is_some() => nested += 1,
            "END" if current.is_some() && nested > 0 => nested -= 1,
            "END" if current.is_some() => {
                if let Some((mut event, duration, date_only)) = current.take() {
                    if event.end.is_none() {
                        event.end = match (event.start, duration, date_only) {
                            (Some(s), Some(d), _) => Some(s + d),
                            (Some(s), None, Some(true)) => Some(s + 86_400),
                            (s, _, _) => s,
                        };
                    }
                    out.push(event);
                }
            }
            _ => {
                let Some((event, duration, date_only)) = current.as_mut().filter(|_| nested == 0)
                else {
                    continue;
                };
                match line.name.as_str() {
                    "UID" => value.clone_into(&mut event.uid),
                    "DTSTART" => {
                        event.start = seconds(value);
                        *date_only = Some(value.len() == 8);
                    }
                    "DTEND" => event.end = seconds(value),
                    "DURATION" => *duration = super::ical::duration(value),
                    "TRANSP" => event.transparent = value.eq_ignore_ascii_case("TRANSPARENT"),
                    "STATUS" => event.status = value.to_ascii_uppercase(),
                    "ORGANIZER" => {
                        event.organizer = Some(address(value));
                        event.organizer_server = is_server_agent(&line);
                    }
                    "ATTENDEE" => event.attendees.push(Attendee {
                        email: address(value),
                        partstat: line
                            .param("PARTSTAT")
                            .unwrap_or("NEEDS-ACTION")
                            .to_ascii_uppercase(),
                        server: is_server_agent(&line),
                    }),
                    _ => {}
                }
            }
        }
    }
    out
}

/// Rewrites an object line by line (unfolded, CRLF): `edit` gets the
/// innermost component and the line and answers `None` to keep it,
/// `Some(None)` to drop it or `Some(Some(line))` to replace it. `BEGIN` /
/// `END` lines are passed too.
pub(super) fn rewrite(
    text: &str,
    mut edit: impl FnMut(&str, &Line) -> Option<Option<Line>>,
) -> String {
    let mut stack: Vec<String> = Vec::new();
    let mut out = String::with_capacity(text.len());
    for raw in unfold(text) {
        let Some(line) = parse_line(&raw) else {
            out.push_str(&raw);
            out.push_str("\r\n");
            continue;
        };
        if line.name == "BEGIN" {
            stack.push(line.value.trim().to_ascii_uppercase());
        }
        let component = stack.last().cloned().unwrap_or_default();
        if line.name == "END" {
            stack.pop();
        }
        match edit(&component, &line) {
            None => {
                out.push_str(&raw);
                out.push_str("\r\n");
            }
            Some(None) => {}
            Some(Some(new)) => {
                out.push_str(&new.text());
                out.push_str("\r\n");
            }
        }
    }
    out
}

/// Sets `METHOD` (replacing one that is there) right after
/// `BEGIN:VCALENDAR`, and removes the server's `SCHEDULE-*` parameters.
pub(super) fn as_itip(text: &str, method: &str) -> String {
    rewrite(text, |component, line| match line.name.as_str() {
        "METHOD" if component == "VCALENDAR" => Some(None),
        "BEGIN" if line.value.trim().eq_ignore_ascii_case("VCALENDAR") => Some(Some(Line {
            name: "BEGIN".into(),
            params: Vec::new(),
            value: format!("VCALENDAR\r\nMETHOD:{method}"),
        })),
        "ORGANIZER" | "ATTENDEE" => {
            let mut new = line.clone();
            new.remove_param("SCHEDULE-STATUS");
            new.remove_param("SCHEDULE-FORCE-SEND");
            Some(Some(new))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_times() {
        let line = parse_line("ATTENDEE;CN=\"A; B\";PARTSTAT=accepted:MAILTO:Bob@X.org").unwrap();
        assert_eq!(line.param("CN"), Some("A; B"));
        assert_eq!(address(&line.value), "bob@x.org");
        assert_eq!(
            line.text(),
            "ATTENDEE;CN=\"A; B\";PARTSTAT=accepted:MAILTO:Bob@X.org"
        );
        assert_eq!(parse_line("nothing"), None);
        assert_eq!(seconds("19700102"), Some(86_400));
        assert_eq!(seconds("19700101T010203Z"), Some(3723));
        assert_eq!(seconds("1970"), None);
        assert_eq!(stamp(1_790_000_000), "20260921T141320Z");
        assert_eq!(seconds(&stamp(1_790_000_000)), Some(1_790_000_000));
        assert_eq!(duration("P1DT1H"), Some(90_000));
        assert_eq!(duration("PT1X"), None);
        assert_eq!(duration("PT1"), None);
    }

    #[test]
    fn events_and_rewrites() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261001T090000Z\r\nDURATION:PT1H\r\n\
                    ORGANIZER;SCHEDULE-AGENT=CLIENT:mailto:ada@x.org\r\nATTENDEE;PARTSTAT=ACCEPTED;SCHEDULE-STATUS=1.2:mailto:bob@x.org\r\n\
                    BEGIN:VALARM\r\nDTSTART:19700101T000000Z\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\n\
                    DTSTART;VALUE=DATE:20261002\r\nTRANSP:TRANSPARENT\r\nSTATUS:tentative\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = events(text);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].end.unwrap() - events[0].start.unwrap(), 3600);
        assert!(!events[0].organizer_server);
        assert_eq!(events[0].attendees[0].partstat, "ACCEPTED");
        assert!(events[0].attendees[0].server);
        assert_eq!(events[1].end.unwrap() - events[1].start.unwrap(), 86_400);
        assert!(events[1].transparent);
        assert_eq!(events[1].status, "TENTATIVE");
        let itip = as_itip(text, "REQUEST");
        assert!(itip.starts_with("BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\n"));
        assert!(!itip.contains("SCHEDULE-STATUS"));
        assert_eq!(as_itip(&itip, "CANCEL").matches("METHOD:").count(), 1);
    }
}
