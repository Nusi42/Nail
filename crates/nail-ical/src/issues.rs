//! Problems found while reading iCalendar and vCard data. Each has a
//! Fluent id in `locales/en/core-ical.ftl` (`ical-*`, `vcard-*`) and turns
//! into the contract's [`PimImportIssue`].

use nail_api::localized::LocalizedMessage;
use nail_api::pim::import::PimImportIssue;

/// What is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueKind {
    /// The data is not iCalendar.
    NotICalendar,
    /// The data is not vCard.
    NotVCard,
    /// A line could not be read (skipped).
    MalformedLine,
    /// A component never ends (closed at the end of the data).
    Unclosed {
        /// `VEVENT`, `VCARD`, ….
        component: String,
    },
    /// An event without `DTSTART` (skipped).
    MissingStart,
    /// A date or time could not be read.
    InvalidTime {
        /// The property (`DTEND`, `EXDATE`, …).
        property: String,
    },
    /// A `TZID` that is neither known nor defined (read as floating).
    UnknownTimeZone {
        /// The `TZID`.
        tzid: String,
    },
    /// A `VTIMEZONE` that matches no IANA zone (times converted to UTC).
    CustomTimeZone {
        /// The `TZID`.
        tzid: String,
    },
    /// A recurrence rule that could not be read (ignored).
    InvalidRecurrence,
    /// An item without `UID` (one was derived from its content).
    MissingUid,
    /// An overridden occurrence without its series (kept as a single
    /// event).
    OrphanOverride,
    /// Two items with the same `UID` (the later one is used).
    DuplicateUid,
    /// Nothing to import.
    NoItems,
    /// An alarm that could not be read (skipped).
    InvalidAlarm,
    /// A contact without any name, email or organization (skipped).
    MissingName,
    /// A contact photo that could not be decoded or is too large
    /// (dropped).
    InvalidPhoto,
    /// No contacts in the data.
    NoContacts,
}

/// A problem with one item or line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// 1-based line, if known.
    pub line: Option<u32>,
    /// The item's title / name or `UID`, if known.
    pub item: Option<String>,
    /// What.
    pub kind: IssueKind,
    /// The item was skipped (else it was read with a fix-up).
    pub skipped: bool,
}

impl Issue {
    /// An issue at a line.
    pub fn new(kind: IssueKind, line: Option<u32>) -> Self {
        Self {
            line,
            item: None,
            kind,
            skipped: false,
        }
    }

    /// Names the item (builder).
    #[must_use]
    pub fn item(mut self, item: Option<String>) -> Self {
        self.item = item.filter(|i| !i.trim().is_empty());
        self
    }

    /// Marks the item as skipped (builder).
    #[must_use]
    pub fn skipped(mut self) -> Self {
        self.skipped = true;
        self
    }

    /// Fluent message id.
    pub fn message_id(&self) -> &'static str {
        match self.kind {
            IssueKind::NotICalendar => "ical-not-icalendar",
            IssueKind::NotVCard => "vcard-not-vcard",
            IssueKind::MalformedLine => "ical-malformed-line",
            IssueKind::Unclosed { .. } => "ical-unclosed-component",
            IssueKind::MissingStart => "ical-missing-start",
            IssueKind::InvalidTime { .. } => "ical-invalid-time",
            IssueKind::UnknownTimeZone { .. } => "ical-unknown-time-zone",
            IssueKind::CustomTimeZone { .. } => "ical-custom-time-zone",
            IssueKind::InvalidRecurrence => "ical-invalid-recurrence",
            IssueKind::MissingUid => "ical-missing-uid",
            IssueKind::OrphanOverride => "ical-orphan-override",
            IssueKind::DuplicateUid => "ical-duplicate-uid",
            IssueKind::NoItems => "ical-no-items",
            IssueKind::InvalidAlarm => "ical-invalid-alarm",
            IssueKind::MissingName => "vcard-missing-name",
            IssueKind::InvalidPhoto => "vcard-invalid-photo",
            IssueKind::NoContacts => "vcard-no-contacts",
        }
    }

    /// The user-facing message (Fluent id + arguments).
    pub fn localized(&self) -> LocalizedMessage {
        let mut message = LocalizedMessage::new(self.message_id());
        if let Some(line) = self.line {
            message = message.arg("line", line);
        }
        if let Some(item) = &self.item {
            message = message.arg("item", item.as_str());
        }
        match &self.kind {
            IssueKind::Unclosed { component } => message.arg("component", component.as_str()),
            IssueKind::InvalidTime { property } => message.arg("property", property.as_str()),
            IssueKind::UnknownTimeZone { tzid } | IssueKind::CustomTimeZone { tzid } => {
                message.arg("tzid", tzid.as_str())
            }
            _ => message,
        }
    }

    /// An English diagnostic (logs, the contract's `message`).
    pub fn diagnostic(&self) -> String {
        let what = match &self.kind {
            IssueKind::NotICalendar => "not iCalendar data".to_owned(),
            IssueKind::NotVCard => "not vCard data".to_owned(),
            IssueKind::MalformedLine => "malformed content line".to_owned(),
            IssueKind::Unclosed { component } => format!("{component} is not closed"),
            IssueKind::MissingStart => "missing DTSTART".to_owned(),
            IssueKind::InvalidTime { property } => format!("invalid date or time in {property}"),
            IssueKind::UnknownTimeZone { tzid } => format!("unknown TZID {tzid:?}"),
            IssueKind::CustomTimeZone { tzid } => {
                format!("VTIMEZONE {tzid:?} matches no IANA zone")
            }
            IssueKind::InvalidRecurrence => "invalid RRULE".to_owned(),
            IssueKind::MissingUid => "missing UID".to_owned(),
            IssueKind::OrphanOverride => "RECURRENCE-ID without its series".to_owned(),
            IssueKind::DuplicateUid => "duplicate UID".to_owned(),
            IssueKind::NoItems => "no events or tasks".to_owned(),
            IssueKind::InvalidAlarm => "invalid VALARM".to_owned(),
            IssueKind::MissingName => "contact without a name".to_owned(),
            IssueKind::InvalidPhoto => "invalid or too large PHOTO".to_owned(),
            IssueKind::NoContacts => "no contacts".to_owned(),
        };
        match self.line {
            Some(line) => format!("line {line}: {what}"),
            None => what,
        }
    }

    /// The contract's form.
    pub fn to_import_issue(&self) -> PimImportIssue {
        PimImportIssue {
            line: self.line,
            item: self.item.clone(),
            message: self.diagnostic(),
            localized: self.localized(),
        }
    }
}

/// The kind and the skipped flag of each issue (what tests compare).
#[cfg(test)]
pub(crate) fn kinds(issues: &[Issue]) -> Vec<(&IssueKind, bool)> {
    issues.iter().map(|i| (&i.kind, i.skipped)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<IssueKind> {
        vec![
            IssueKind::NotICalendar,
            IssueKind::NotVCard,
            IssueKind::MalformedLine,
            IssueKind::Unclosed {
                component: "VEVENT".into(),
            },
            IssueKind::MissingStart,
            IssueKind::InvalidTime {
                property: "DTEND".into(),
            },
            IssueKind::UnknownTimeZone { tzid: "X".into() },
            IssueKind::CustomTimeZone { tzid: "Y".into() },
            IssueKind::InvalidRecurrence,
            IssueKind::MissingUid,
            IssueKind::OrphanOverride,
            IssueKind::DuplicateUid,
            IssueKind::NoItems,
            IssueKind::InvalidAlarm,
            IssueKind::MissingName,
            IssueKind::InvalidPhoto,
            IssueKind::NoContacts,
        ]
    }

    #[test]
    fn every_issue_has_an_english_message_and_an_empty_german_file() {
        let en = include_str!("../../../locales/en/core-ical.ftl");
        let de = include_str!("../../../locales/de/core-ical.ftl");
        let ids: std::collections::HashSet<&str> = en
            .lines()
            .filter_map(|l| l.split_once(" = ").map(|(id, _)| id.trim()))
            .collect();
        let mut seen = std::collections::HashSet::new();
        for kind in all() {
            let issue = Issue::new(kind, Some(3)).item(Some("Standup".into()));
            let id = issue.message_id();
            assert!(ids.contains(id), "{id} missing in core-ical.ftl");
            assert!(seen.insert(id), "{id} used twice");
            let localized = issue.localized();
            assert!(localized.is_valid());
            // Every argument the message passes is used by the English text.
            let text = en
                .lines()
                .find(|l| l.starts_with(&format!("{id} =")))
                .unwrap();
            for name in localized.args.keys() {
                if name != "line" && name != "item" {
                    assert!(text.contains(&format!("${name}")), "{id} lacks ${name}");
                }
            }
            assert_eq!(issue.to_import_issue().line, Some(3));
            assert!(issue.diagnostic().starts_with("line 3: "));
        }
        assert_eq!(seen.len(), ids.len(), "unused ids in core-ical.ftl");
        assert!(
            de.lines()
                .all(|l| l.trim().is_empty() || l.starts_with('#')),
            "German catalog stays empty"
        );
    }

    #[test]
    fn empty_item_names_are_dropped() {
        let issue = Issue::new(IssueKind::MissingUid, None).item(Some("  ".into()));
        assert_eq!(issue.item, None);
        assert!(!issue.skipped);
        assert!(issue.clone().skipped().skipped);
        assert!(!issue.localized().args.contains_key("line"));
    }
}
