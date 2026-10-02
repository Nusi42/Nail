//! The store / data traits the core implements: importers push into
//! sinks, exporters and backups pull from sources. This crate never
//! touches the store, the operation queues or the configuration itself.

use nail_api::Flags;
use nail_api::domain::FolderRole;
use nail_api::portability::{ImportCollectionKind, ItemRef};
use nail_ical::vcard::{ParsedContact, ParsedGroup};
use nail_ical::{ParsedEvent, ParsedTask};

use crate::error::SinkError;

/// A mail folder of an import source, in source order (parents first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFolder {
    /// Its path inside the source, `/`-separated (the preview's key and
    /// [`nail_api::portability::ImportOptions::folders`] entry).
    pub path: String,
    /// The name to create (sanitized: no `/`, no control characters).
    pub name: String,
    /// The parent's path, `None` for top-level folders.
    pub parent: Option<String>,
    /// The special-use role guessed from the name (`Inbox`, `Sent`, …);
    /// the core may use it when the target has no folder with that role.
    pub role: FolderRole,
}

/// The duplicate check of `skip_duplicates`: `Message-ID`, date and size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DuplicateKey {
    /// `Message-ID` without angle brackets, if any.
    pub message_id: Option<String>,
    /// `Date` as a Unix timestamp, if readable.
    pub date: Option<i64>,
    /// Size of the raw message in bytes.
    pub size: u64,
}

/// One message read from a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedMessage {
    /// The raw RFC 5322 message (mbox quoting undone, `From ` line removed).
    pub raw: Vec<u8>,
    /// System flags (empty unless the source had them and `keep_flags`).
    pub flags: Flags,
    /// Keywords / tags (`$Label1`, `work`), deduplicated.
    pub keywords: Vec<String>,
    /// When the message arrived (Unix seconds): Maildir file time, mbox
    /// `From ` line date; `None` = use the `Date` header.
    pub received_at: Option<i64>,
    /// The duplicate check.
    pub key: DuplicateKey,
    /// Where it came from (for issues the sink reports).
    pub origin: ItemRef,
}

/// Where the core puts imported mail: "Local Folders" or a folder of an
/// account (then every message is an `AppendMessage` through the queue).
pub trait MailImportSink {
    /// Makes sure the folder exists in the target (parents come first).
    fn ensure_folder(&mut self, folder: &SourceFolder) -> Result<(), SinkError>;

    /// Whether a message with this key is already in the folder (asked
    /// only with `skip_duplicates`).
    fn contains(&mut self, folder: &SourceFolder, key: &DuplicateKey) -> Result<bool, SinkError>;

    /// Adds a message to the folder.
    fn append(&mut self, folder: &SourceFolder, message: ImportedMessage) -> Result<(), SinkError>;
}

/// A calendar, task list or address book of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCollection {
    /// The preview's key ([`nail_api::portability::ImportOptions::collections`]).
    pub key: String,
    /// Display name.
    pub name: String,
    /// What it holds.
    pub kind: ImportCollectionKind,
}

/// Where the core puts imported calendars, tasks and contacts: through
/// the PIM queue into the chosen calendar / address book (same `UID` =
/// update).
pub trait PimImportSink {
    /// Adds or updates an event series.
    fn event(&mut self, collection: &SourceCollection, event: ParsedEvent)
    -> Result<(), SinkError>;
    /// Adds or updates a task.
    fn task(&mut self, collection: &SourceCollection, task: ParsedTask) -> Result<(), SinkError>;
    /// Adds or updates a contact.
    fn contact(
        &mut self,
        collection: &SourceCollection,
        contact: ParsedContact,
    ) -> Result<(), SinkError>;
    /// Adds or updates a contact group.
    fn group(&mut self, collection: &SourceCollection, group: ParsedGroup)
    -> Result<(), SinkError>;
    /// Whether an item with this `UID` exists in the target (restore plans
    /// count updates with it; default: unknown = no).
    fn exists(&mut self, _collection: &SourceCollection, _uid: &str) -> bool {
        false
    }
}

/// A sink that drops everything (previews; counting).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl MailImportSink for NullSink {
    fn ensure_folder(&mut self, _folder: &SourceFolder) -> Result<(), SinkError> {
        Ok(())
    }
    fn contains(&mut self, _folder: &SourceFolder, _key: &DuplicateKey) -> Result<bool, SinkError> {
        Ok(false)
    }
    fn append(
        &mut self,
        _folder: &SourceFolder,
        _message: ImportedMessage,
    ) -> Result<(), SinkError> {
        Ok(())
    }
}

impl PimImportSink for NullSink {
    fn event(&mut self, _c: &SourceCollection, _e: ParsedEvent) -> Result<(), SinkError> {
        Ok(())
    }
    fn task(&mut self, _c: &SourceCollection, _t: ParsedTask) -> Result<(), SinkError> {
        Ok(())
    }
    fn contact(&mut self, _c: &SourceCollection, _p: ParsedContact) -> Result<(), SinkError> {
        Ok(())
    }
    fn group(&mut self, _c: &SourceCollection, _g: ParsedGroup) -> Result<(), SinkError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Export sources

/// A mail folder to export, relative to the export (`Inbox`,
/// `Inbox/2024`; parents first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFolder {
    /// The path below the export root, `/`-separated.
    pub path: String,
    /// Messages in it (for progress), if known.
    pub messages: Option<u64>,
    /// Bytes in it (for progress), if known.
    pub size_bytes: Option<u64>,
}

impl ExportFolder {
    /// A folder without known counts.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            messages: None,
            size_bytes: None,
        }
    }

    /// The last path segment.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The parent's path.
    pub fn parent(&self) -> Option<&str> {
        self.path.rsplit_once('/').map(|(parent, _)| parent)
    }
}

/// One message to export.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportMessage {
    /// The raw RFC 5322 message.
    pub raw: Vec<u8>,
    /// System flags.
    pub flags: Flags,
    /// Keywords / tags.
    pub keywords: Vec<String>,
    /// When it arrived (Unix seconds), for the mbox `From ` line and the
    /// Maildir file time; `None` = the `Date` header / now.
    pub received_at: Option<i64>,
    /// The envelope sender for the mbox `From ` line; `None` = from the
    /// `Return-Path` / `From` header.
    pub envelope_from: Option<String>,
}

/// Whether an export source visitor goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visit {
    /// Give the next message.
    Continue,
    /// Stop (cancelled).
    Stop,
}

/// The core's mail, read for an export or a backup.
pub trait MailExportSource {
    /// The folders, parents first.
    fn folders(&mut self) -> Result<Vec<ExportFolder>, SinkError>;

    /// Calls `visit` for every message of `folder` in order until it
    /// answers [`Visit::Stop`]. A message that cannot be read is passed as
    /// `Err` (reported, the export goes on).
    fn messages(
        &mut self,
        folder: &ExportFolder,
        visit: &mut dyn FnMut(Result<ExportMessage, SinkError>) -> Visit,
    ) -> Result<(), SinkError>;
}

/// A calendar, task list or address book to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportCollection {
    /// A key the source understands (the calendar / address book id).
    pub key: String,
    /// Display name (`X-WR-CALNAME`; the file name).
    pub name: String,
    /// Items (for progress), if known.
    pub items: Option<u64>,
}

/// The core's calendars and address books, read for an export or a
/// backup. Items are passed as text: each event / task as a whole
/// `VCALENDAR` (as `nail_ical::write::write_event` / `write_task` produce
/// it, or the stored original), each contact / group as a `VCARD`.
pub trait PimExportSource {
    /// The collections.
    fn collections(&mut self) -> Result<Vec<ExportCollection>, SinkError>;

    /// Calls `visit` with the text of every item of the collection.
    fn items(
        &mut self,
        collection: &ExportCollection,
        visit: &mut dyn FnMut(Result<String, SinkError>) -> Visit,
    ) -> Result<(), SinkError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_folder_names() {
        let f = ExportFolder::new("Inbox/2024/Q1");
        assert_eq!(f.name(), "Q1");
        assert_eq!(f.parent(), Some("Inbox/2024"));
        assert_eq!(ExportFolder::new("Inbox").parent(), None);
    }

    #[test]
    fn the_null_sink_accepts_everything() {
        let folder = SourceFolder {
            path: "a".into(),
            name: "a".into(),
            parent: None,
            role: FolderRole::Other,
        };
        let mut sink = NullSink;
        assert!(sink.ensure_folder(&folder).is_ok());
        let key = DuplicateKey {
            message_id: None,
            date: None,
            size: 0,
        };
        assert_eq!(sink.contains(&folder, &key), Ok(false));
        let collection = SourceCollection {
            key: "k".into(),
            name: "n".into(),
            kind: ImportCollectionKind::Calendar,
        };
        assert!(!PimImportSink::exists(&mut sink, &collection, "uid"));
    }
}
