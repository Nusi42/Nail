//! A small iCalendar content-line layer (RFC 5545 §3.1) for scheduling:
//! unfolding, parameters (quoted, multi-valued), folding, and patching
//! single lines while every other line stays byte for byte as it was.
//!
//! `nail-ical` is Nail's full iCalendar model; scheduling only needs to
//! read and write a handful of properties (`FREEBUSY`, `ATTENDEE`,
//! `ORGANIZER`, `METHOD`, `UID`, the `SCHEDULE-AGENT` / `SCHEDULE-STATUS`
//! parameters), so `nail-dav` keeps to this layer and does not depend on
//! the whole model.

use std::fmt::Write as _;

/// Longest line Nail writes before folding (octets, without CRLF).
pub const FOLD_AT: usize = 75;

/// One unfolded content line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    /// Property name, upper-case.
    pub name: String,
    /// Parameters in order: upper-case name and the raw value (quotes
    /// removed, several values still comma-separated).
    pub params: Vec<(String, String)>,
    /// The value as written (escapes are not resolved).
    pub value: String,
}

impl ContentLine {
    /// A line without parameters.
    pub fn new(name: &str, value: impl Into<String>) -> Self {
        Self {
            name: name.to_ascii_uppercase(),
            params: Vec::new(),
            value: value.into(),
        }
    }

    /// Adds a parameter (builder style).
    #[must_use]
    pub fn with_param(mut self, name: &str, value: &str) -> Self {
        self.set_param(name, value);
        self
    }

    /// The first value of parameter `name` (case-insensitive).
    pub fn param(&self, name: &str) -> Option<&str> {
        nail_util::named::first(&self.params, name)
    }

    /// Sets (replaces) parameter `name`.
    pub fn set_param(&mut self, name: &str, value: &str) {
        let name = name.to_ascii_uppercase();
        if let Some(slot) = self.params.iter_mut().find(|(n, _)| *n == name) {
            value.clone_into(&mut slot.1);
        } else {
            self.params.push((name, value.to_owned()));
        }
    }

    /// Removes parameter `name`; whether it was there.
    pub fn remove_param(&mut self, name: &str) -> bool {
        let before = self.params.len();
        self.params.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        before != self.params.len()
    }

    /// Parses one unfolded line; `None` when it has no `:` outside quotes
    /// or no name.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut in_quotes = false;
        let mut colon = None;
        for (i, c) in line.char_indices() {
            match c {
                '"' => in_quotes = !in_quotes,
                ':' if !in_quotes => {
                    colon = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let colon = colon?;
        let head = &line[..colon];
        let value = line[colon + 1..].to_owned();
        let mut parts = split_unquoted(head, ';').into_iter();
        let name = parts.next()?.trim().to_ascii_uppercase();
        if name.is_empty() {
            return None;
        }
        let mut params = Vec::new();
        for part in parts {
            let (key, raw) = part.split_once('=').unwrap_or((part, ""));
            let unquoted: String = split_unquoted(raw, ',')
                .into_iter()
                .map(|v| v.trim().trim_matches('"').to_owned())
                .collect::<Vec<_>>()
                .join(",");
            params.push((key.trim().to_ascii_uppercase(), unquoted));
        }
        Some(Self {
            name,
            params,
            value,
        })
    }

    /// The line as text, folded at [`FOLD_AT`] octets, ending in CRLF.
    pub fn to_text(&self) -> String {
        let mut line = self.name.clone();
        for (name, value) in &self.params {
            let _ = write!(line, ";{name}=");
            let values: Vec<String> = value
                .split(',')
                .map(|v| {
                    if v.contains([';', ':', ',']) || v.is_empty() {
                        format!("\"{}\"", v.replace('"', ""))
                    } else {
                        v.to_owned()
                    }
                })
                .collect();
            line.push_str(&values.join(","));
        }
        line.push(':');
        line.push_str(&self.value);
        fold(&line)
    }
}

/// Splits on `sep` outside double quotes.
fn split_unquoted(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == sep && !in_quotes {
            out.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&text[start..]);
    out
}

/// Folds one logical line at [`FOLD_AT`] octets (never inside a UTF-8
/// sequence) and ends it with CRLF.
pub fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / FOLD_AT * 3 + 2);
    let mut width = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if width + len > FOLD_AT {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
    out
}

/// A logical line with the physical text it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    /// The original text including folds and the line break.
    pub raw: String,
    /// The unfolded text without the line break.
    pub unfolded: String,
}

/// Splits a document into logical lines (CRLF or bare LF; a line starting
/// with a space or tab continues the previous one).
pub fn logical_lines(text: &str) -> Vec<RawLine> {
    let mut out: Vec<RawLine> = Vec::new();
    for physical in text.split_inclusive('\n') {
        let content = physical.trim_end_matches('\n').trim_end_matches('\r');
        if let Some(rest) = content.strip_prefix([' ', '\t'])
            && let Some(last) = out.last_mut()
        {
            last.raw.push_str(physical);
            last.unfolded.push_str(rest);
            continue;
        }
        if content.is_empty() {
            // Blank lines carry nothing; keep them verbatim.
            out.push(RawLine {
                raw: physical.to_owned(),
                unfolded: String::new(),
            });
            continue;
        }
        out.push(RawLine {
            raw: physical.to_owned(),
            unfolded: content.to_owned(),
        });
    }
    out
}

/// A parsed line with the name of the component it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// The innermost component (`VEVENT`, `VFREEBUSY`, `VCALENDAR`, …);
    /// empty outside any.
    pub component: String,
    /// The line.
    pub line: ContentLine,
}

/// Every content line with its component (`BEGIN` / `END` lines are left
/// out). Unparsable lines are skipped.
pub fn located_lines(text: &str) -> Vec<Located> {
    let mut stack: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for raw in logical_lines(text) {
        let Some(line) = ContentLine::parse(&raw.unfolded) else {
            continue;
        };
        match line.name.as_str() {
            "BEGIN" => stack.push(line.value.trim().to_ascii_uppercase()),
            "END" => {
                stack.pop();
            }
            _ => out.push(Located {
                component: stack.last().cloned().unwrap_or_default(),
                line,
            }),
        }
    }
    out
}

/// Rewrites the lines `edit` changes and keeps every other line verbatim.
/// `edit` gets the component name and the parsed line and returns
/// `Some(new)` to replace it (`None` keeps it). Returns the new text and
/// whether anything changed.
pub fn patch_lines(
    text: &str,
    mut edit: impl FnMut(&str, &ContentLine) -> Option<ContentLine>,
) -> (String, bool) {
    let mut stack: Vec<String> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for raw in logical_lines(text) {
        let Some(line) = ContentLine::parse(&raw.unfolded) else {
            out.push_str(&raw.raw);
            continue;
        };
        match line.name.as_str() {
            "BEGIN" => stack.push(line.value.trim().to_ascii_uppercase()),
            "END" => {
                stack.pop();
            }
            _ => {
                let component = stack.last().map_or("", String::as_str);
                if let Some(new) = edit(component, &line)
                    && new != line
                {
                    out.push_str(&new.to_text());
                    changed = true;
                    continue;
                }
            }
        }
        out.push_str(&raw.raw);
    }
    (out, changed)
}

/// The address of a `mailto:` calendar-user URI (`MAILTO:` too), without
/// surrounding space; other URIs come back as they are.
pub fn address_of(uri: &str) -> &str {
    let uri = uri.trim();
    match uri.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("mailto:") => uri[7..].trim(),
        _ => uri,
    }
}

/// Whether two calendar-user addresses are the same (`mailto:` and case
/// ignored).
pub fn same_address(a: &str, b: &str) -> bool {
    address_of(a).eq_ignore_ascii_case(address_of(b))
}

/// Escapes a TEXT value (RFC 5545 §3.3.11).
pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_names_params_and_values() {
        let line = ContentLine::parse(
            "attendee;CN=\"Doe; John\";SCHEDULE-STATUS=\"1.2,3.7\";ROLE=REQ-PARTICIPANT:mailto:j@x.org",
        )
        .unwrap();
        assert_eq!(line.name, "ATTENDEE");
        assert_eq!(line.param("cn"), Some("Doe; John"));
        assert_eq!(line.param("SCHEDULE-STATUS"), Some("1.2,3.7"));
        assert_eq!(line.value, "mailto:j@x.org");
        let quoted_colon = ContentLine::parse("X-A;P=\"a:b\":v:w").unwrap();
        assert_eq!(quoted_colon.param("P"), Some("a:b"));
        assert_eq!(quoted_colon.value, "v:w");
        assert_eq!(ContentLine::parse("no colon"), None);
        assert_eq!(ContentLine::parse(":value"), None);
        let flag = ContentLine::parse("X;RSVP:v").unwrap();
        assert_eq!(flag.param("RSVP"), Some(""));
    }

    #[test]
    fn serializes_with_quotes_and_folding() {
        let mut line = ContentLine::new("attendee", "mailto:j@x.org")
            .with_param("CN", "Doe; John")
            .with_param("PARTSTAT", "ACCEPTED");
        assert_eq!(
            line.to_text(),
            "ATTENDEE;CN=\"Doe; John\";PARTSTAT=ACCEPTED:mailto:j@x.org\r\n"
        );
        line.set_param("partstat", "DECLINED");
        assert_eq!(line.param("PARTSTAT"), Some("DECLINED"));
        assert!(line.remove_param("Cn"));
        assert!(!line.remove_param("CN"));
        let long = ContentLine::new("DESCRIPTION", "ä".repeat(60)).to_text();
        for physical in long.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(physical.len() <= FOLD_AT, "{physical:?}");
        }
        let back = logical_lines(&long);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].unfolded, format!("DESCRIPTION:{}", "ä".repeat(60)));
    }

    #[test]
    fn unfolds_and_locates_lines() {
        let text = "BEGIN:VCALENDAR\r\nMETHOD:REPLY\r\nBEGIN:VFREEBUSY\r\nFREEBUSY:2026\r\n 1001T090000Z/PT1H\nEND:VFREEBUSY\r\n\r\nEND:VCALENDAR\r\n";
        let lines = located_lines(text);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].component, "VCALENDAR");
        assert_eq!(lines[0].line.value, "REPLY");
        assert_eq!(lines[1].component, "VFREEBUSY");
        assert_eq!(lines[1].line.value, "20261001T090000Z/PT1H");
    }

    #[test]
    fn patches_only_changed_lines() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:a\r\n  folded\r\nATTENDEE:mailto:b@x.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (same, changed) = patch_lines(text, |_, _| None);
        assert!(!changed);
        assert_eq!(same, text);
        let (out, changed) = patch_lines(text, |component, line| {
            (component == "VEVENT" && line.name == "ATTENDEE")
                .then(|| line.clone().with_param("PARTSTAT", "ACCEPTED"))
        });
        assert!(changed);
        assert!(out.contains("SUMMARY:a\r\n  folded\r\n"));
        assert!(out.contains("ATTENDEE;PARTSTAT=ACCEPTED:mailto:b@x.org\r\n"));
    }

    #[test]
    fn addresses_and_text() {
        assert_eq!(address_of(" MAILTO:Ada@X.org "), "Ada@X.org");
        assert_eq!(address_of("urn:uuid:1"), "urn:uuid:1");
        assert!(same_address("mailto:ada@x.org", "ADA@x.org"));
        assert!(!same_address("mailto:ada@x.org", "bob@x.org"));
        assert_eq!(escape_text("a;b,c\\d\r\ne"), "a\\;b\\,c\\\\d\\ne");
    }
}
