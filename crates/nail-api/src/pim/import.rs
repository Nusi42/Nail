//! Importing `.ics` and `.vcf` data (files opened with Nail, files picked
//! in the UI, or text uploaded by the browser).
//!
//! The UI first asks for a preview ([`crate::Request::PreviewPimImport`]):
//! what the data holds and what is wrong with it. The import itself
//! ([`crate::Request::ImportPim`]) writes into one calendar and / or one
//! address book through the PIM queue; items whose `UID` already exists
//! there are updated, not duplicated.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::OpId;
use crate::localized::LocalizedMessage;

/// Largest import Nail reads, bytes.
pub const MAX_PIM_IMPORT_BYTES: usize = 32 * 1024 * 1024;

/// Where import data comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PimImportSource {
    /// A file on the desktop (opened with Nail or picked in the desktop
    /// app). Desktop only: remote sessions are refused.
    File {
        /// Absolute path.
        path: String,
    },
    /// Text uploaded by the UI (browser file picker, paste).
    Data {
        /// The original file name, if any (for messages).
        file_name: Option<String>,
        /// The `.ics` / `.vcf` text.
        content: String,
    },
}

/// What kind of data an import holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimFormat {
    /// iCalendar (`.ics`).
    Icalendar,
    /// vCard (`.vcf`).
    Vcard,
}

/// A problem found while reading import data. Items with problems are
/// skipped; the rest is imported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimImportIssue {
    /// Line of the data (1-based), if known.
    pub line: Option<u32>,
    /// The item's title / name or `UID`, if known.
    pub item: Option<String>,
    /// Diagnostic detail (English).
    pub message: String,
    /// The user-facing text (Fluent id in `core-ical.ftl`).
    pub localized: LocalizedMessage,
}

/// What an import would do ([`crate::Request::PreviewPimImport`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimImportPreview {
    /// The data's format.
    pub format: PimFormat,
    /// The calendar name the data gives (`X-WR-CALNAME`), if any.
    pub name: Option<String>,
    /// Events found (series count once).
    pub events: u32,
    /// Tasks found.
    pub tasks: u32,
    /// Contacts found.
    pub contacts: u32,
    /// Contact groups found.
    pub groups: u32,
    /// Titles / names of the first items (at most 20), for the dialog.
    pub sample: Vec<String>,
    /// Problems; affected items are skipped.
    pub issues: Vec<PimImportIssue>,
}

/// What an import did ([`crate::Request::ImportPim`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimImportResult {
    /// Items created.
    pub created: u32,
    /// Items updated (same `UID` already there).
    pub updated: u32,
    /// Items skipped (problems, or read-only targets).
    pub skipped: u32,
    /// Problems.
    pub issues: Vec<PimImportIssue>,
    /// The queued operations.
    pub op_ids: Vec<OpId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sources_and_results_serialize() {
        let source = PimImportSource::Data {
            file_name: Some("team.vcf".into()),
            content: "BEGIN:VCARD\r\nEND:VCARD\r\n".into(),
        };
        let value = serde_json::to_value(&source).unwrap();
        assert_eq!(value["type"], "data");
        assert_eq!(
            serde_json::from_value::<PimImportSource>(value).unwrap(),
            source
        );
        let issue = PimImportIssue {
            line: Some(3),
            item: None,
            message: "missing DTSTART".into(),
            localized: LocalizedMessage::new("ical-missing-start"),
        };
        assert_eq!(
            serde_json::to_value(&issue).unwrap(),
            json!({"line": 3, "item": null, "message": "missing DTSTART", "localized": {"id": "ical-missing-start"}})
        );
        assert_eq!(
            serde_json::to_value(PimFormat::Icalendar).unwrap(),
            json!("icalendar")
        );
    }
}
