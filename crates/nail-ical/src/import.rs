//! Import previews and reading of `.ics` / `.vcf` data
//! ([`nail_api::Request::PreviewPimImport`] / `ImportPim`).

use nail_api::pim::import::{PimFormat, PimImportPreview};

use crate::content;
use crate::ical::{ParseOptions, ParsedCalendar, parse_calendar};
use crate::issues::{Issue, IssueKind};
use crate::vcard::{ParsedCards, parse_cards};

/// Most titles in a preview's sample.
pub const SAMPLE_SIZE: usize = 20;

/// What import data holds, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportData {
    /// iCalendar.
    Calendar(ParsedCalendar),
    /// vCard.
    Cards(ParsedCards),
}

/// The format of import data: by content (`BEGIN:VCALENDAR` /
/// `BEGIN:VCARD` first), else by the file name's extension.
pub fn detect_format(text: &str, file_name: Option<&str>) -> Option<PimFormat> {
    let first = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_ascii_uppercase);
    match first.as_deref() {
        Some("BEGIN:VCALENDAR") => return Some(PimFormat::Icalendar),
        Some("BEGIN:VCARD") => return Some(PimFormat::Vcard),
        _ => {}
    }
    let extension = file_name?.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "ics" | "ical" | "icalendar" | "ifb" => Some(PimFormat::Icalendar),
        "vcf" | "vcard" => Some(PimFormat::Vcard),
        _ => None,
    }
}

/// Reads import text.
pub fn read(text: &str, file_name: Option<&str>) -> ImportData {
    match detect_format(text, file_name) {
        Some(PimFormat::Vcard) => ImportData::Cards(parse_cards(text)),
        _ => ImportData::Calendar(parse_calendar(text, ParseOptions::default())),
    }
}

/// Reads import bytes (a file: UTF-8, UTF-16 with BOM, else
/// Windows-1252).
pub fn read_bytes(bytes: &[u8], file_name: Option<&str>) -> ImportData {
    read(&content::decode_bytes(bytes).0, file_name)
}

/// The preview of import data.
pub fn preview(data: &ImportData) -> PimImportPreview {
    match data {
        ImportData::Calendar(calendar) => {
            let sample = calendar
                .events
                .iter()
                .map(|e| e.fields.title.clone())
                .chain(calendar.tasks.iter().map(|t| t.fields.title.clone()))
                .filter(|t| !t.trim().is_empty())
                .take(SAMPLE_SIZE)
                .collect();
            PimImportPreview {
                format: PimFormat::Icalendar,
                name: calendar.name.clone(),
                events: count(calendar.events.len()),
                tasks: count(calendar.tasks.len()),
                contacts: 0,
                groups: 0,
                sample,
                issues: calendar.issues.iter().map(Issue::to_import_issue).collect(),
            }
        }
        ImportData::Cards(cards) => PimImportPreview {
            format: PimFormat::Vcard,
            name: None,
            events: 0,
            tasks: 0,
            contacts: count(cards.contacts.len()),
            groups: count(cards.groups.len()),
            sample: cards
                .contacts
                .iter()
                .map(|c| c.fields.display_name.clone())
                .chain(cards.groups.iter().map(|g| g.group.name.clone()))
                .take(SAMPLE_SIZE)
                .collect(),
            issues: cards.issues.iter().map(Issue::to_import_issue).collect(),
        },
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Whether the data holds nothing importable (only problems).
pub fn is_empty(data: &ImportData) -> bool {
    match data {
        ImportData::Calendar(c) => c.events.is_empty() && c.tasks.is_empty(),
        ImportData::Cards(c) => c.contacts.is_empty() && c.groups.is_empty(),
    }
}

/// Issues that make the whole import fail (not iCalendar / vCard).
pub fn fatal_issue(data: &ImportData) -> Option<&Issue> {
    let issues = match data {
        ImportData::Calendar(c) => &c.issues,
        ImportData::Cards(c) => &c.issues,
    };
    issues
        .iter()
        .find(|i| matches!(i.kind, IssueKind::NotICalendar | IssueKind::NotVCard))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_detected() {
        assert_eq!(
            detect_format("\u{feff}\r\nBEGIN:VCALENDAR\r\n", None),
            Some(PimFormat::Icalendar)
        );
        assert_eq!(detect_format("begin:vcard\n", None), Some(PimFormat::Vcard));
        assert_eq!(
            detect_format("garbage", Some("x.VCF")),
            Some(PimFormat::Vcard)
        );
        assert_eq!(
            detect_format("garbage", Some("x.ics")),
            Some(PimFormat::Icalendar)
        );
        assert_eq!(detect_format("garbage", Some("x.txt")), None);
        assert_eq!(detect_format("garbage", None), None);
    }

    #[test]
    fn previews() {
        let data = read(
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Team\r\nBEGIN:VEVENT\r\nUID:1\r\nDTSTART:20261001T090000Z\r\nSUMMARY:One\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:1\r\nRECURRENCE-ID:20261002T090000Z\r\nDTSTART:20261002T100000Z\r\nSUMMARY:One moved\r\nEND:VEVENT\r\n\
BEGIN:VTODO\r\nUID:2\r\nSUMMARY:Two\r\nEND:VTODO\r\nBEGIN:VEVENT\r\nUID:3\r\nSUMMARY:No start\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            Some("team.ics"),
        );
        let p = preview(&data);
        assert_eq!((p.format, p.events, p.tasks), (PimFormat::Icalendar, 1, 1));
        assert_eq!(p.name.as_deref(), Some("Team"));
        assert_eq!(p.sample, ["One", "Two"]);
        assert_eq!(p.issues.len(), 1);
        assert_eq!(p.issues[0].localized.id, "ical-missing-start");
        assert_eq!(p.issues[0].item.as_deref(), Some("No start"));
        assert!(!is_empty(&data) && fatal_issue(&data).is_none());

        let cards = read_bytes(
            b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:A\r\nUID:a\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\nFN:G\r\nUID:g\r\nEND:VCARD\r\n",
            Some("x.vcf"),
        );
        let p = preview(&cards);
        assert_eq!((p.format, p.contacts, p.groups), (PimFormat::Vcard, 1, 1));
        assert_eq!(p.sample, ["A", "G"]);

        let bad = read("hello", Some("x.vcf"));
        assert!(is_empty(&bad));
        assert_eq!(
            fatal_issue(&bad).map(|i| i.message_id()),
            Some("vcard-not-vcard")
        );
        let bad = read("hello", None);
        assert_eq!(
            fatal_issue(&bad).map(|i| i.message_id()),
            Some("ical-not-icalendar")
        );
    }
}
