//! Data portability (M4c, ADR 0094): importing mail, calendars and
//! contacts from other programs, exporting them, and the job model that
//! backups, verifications and restores ([`crate::backup`]) share.
//!
//! # Flow
//!
//! 1. The UI finds candidates ([`crate::Request::FindImportSources`]:
//!    Thunderbird profiles from `profiles.ini`) or the user picks a file /
//!    folder ([`crate::Request::PickPath`] on the desktop, a typed path
//!    otherwise).
//! 2. [`crate::Request::PreviewImport`] reads the source without writing
//!    anything: folders with message counts, calendars / address books,
//!    Thunderbird account and filter proposals, and the first problems.
//! 3. [`crate::Request::StartImport`] / [`crate::Request::StartExport`]
//!    start a job ([`JobStatus`]); progress arrives as
//!    [`crate::Event::PortabilityJobProgress`] and the end as
//!    [`crate::Event::PortabilityJobFinished`] with a [`JobReport`]. Jobs
//!    run one at a time per kind of target and can be cancelled.
//!
//! Paths are paths on the computer running the core, so every request
//! naming one is desktop-only for remote sessions (`forbidden` there).
//!
//! # Where imported data goes
//!
//! Mail goes into the local "Local Folders" account
//! ([`LOCAL_FOLDERS_ACCOUNT_ID`], created on first use, never synced) by
//! default, or into a folder of a real account — then every message is an
//! `AppendMessage` through that account's operation queue, like any other
//! write. Calendars, tasks and contacts go through the PIM queue into the
//! chosen calendar / address book (items whose `UID` exists are updated).
//! Thunderbird accounts and filters are only **proposed**
//! ([`AccountProposal`], [`RuleProposal`]): the user reviews them and the
//! UI saves the chosen ones with the ordinary `AddAccount` / `SaveRule`
//! requests (passwords are never imported).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::AccountConfig;
use crate::backup::{BackupManifest, RestoreReport};
use crate::error::ErrorCode;
use crate::ids::{AccountId, AddressBookId, CalendarId, FolderId, JobId, MessageId};
use crate::localized::LocalizedMessage;
use crate::rules::Rule;

/// The local account imported mail goes into by default ("Local
/// Folders"); created by the core on first use, never synced, included in
/// backups as [`crate::backup::BackupItem::LocalMail`].
pub const LOCAL_FOLDERS_ACCOUNT_ID: &str = "local-folders";

/// Most item issues a [`JobReport`] keeps; later ones are only counted
/// ([`JobReport::issues_truncated`]).
pub const MAX_JOB_ISSUES: usize = 1000;

/// Most item issues an [`ImportPreview`] lists.
pub const MAX_PREVIEW_ISSUES: usize = 100;

/// Largest single message an import reads (bigger ones are skipped with
/// [`PortabilityIssueCode::TooLarge`]); the same limit as opened files.
pub const MAX_IMPORT_MESSAGE_BYTES: u64 = 64 * 1024 * 1024;

/// How long finished jobs stay listed ([`crate::Request::ListPortabilityJobs`]).
pub const FINISHED_JOB_RETENTION_SECS: u64 = 24 * 60 * 60;

labeled_enum! {
    /// What an import reads (`import-format-*`).
    pub enum ImportFormat : "import-format" {
        /// One mbox file (mboxrd and mboxo; `From ` lines, streamed).
        Mbox = "mbox",
        /// A folder of `.eml` files (subfolders become folders).
        EmlFolder = "eml-folder",
        /// A Maildir (`cur` / `new` / `tmp`; Courier / Dovecot `.Sub.Folder`
        /// trees and `:2,` flags).
        Maildir = "maildir",
        /// A Thunderbird profile folder: local folders (mbox + `.msf`
        /// ignored), address books (`abook.sqlite`, Mork `.mab` where it
        /// can be read), accounts (`prefs.js`) and filters
        /// (`msgFilterRules.dat`) as proposals.
        ThunderbirdProfile = "thunderbird-profile",
        /// An iCalendar file (events and tasks).
        Ics = "ics",
        /// A vCard file (contacts and groups).
        Vcard = "vcard",
    }
}

impl ImportFormat {
    /// Whether the source is a folder (else a file).
    pub fn is_directory(self) -> bool {
        matches!(
            self,
            Self::EmlFolder | Self::Maildir | Self::ThunderbirdProfile
        )
    }

    /// Whether the source can hold mail.
    pub fn has_mail(self) -> bool {
        !matches!(self, Self::Ics | Self::Vcard)
    }

    /// Whether the source can hold calendars, tasks or contacts.
    pub fn has_pim(self) -> bool {
        matches!(self, Self::Ics | Self::Vcard | Self::ThunderbirdProfile)
    }
}

labeled_enum! {
    /// What an export writes (`export-format-*`).
    pub enum ExportFormat : "export-format" {
        /// One mbox file per folder (mboxrd by default).
        Mbox = "mbox",
        /// One `.eml` file per message, a folder per folder.
        Eml = "eml",
        /// A Maildir tree (`:2,` flags).
        Maildir = "maildir",
        /// One iCalendar file per calendar / task list.
        Ics = "ics",
        /// One vCard 4 file per address book.
        Vcard = "vcard",
    }
}

impl ExportFormat {
    /// Whether this format can write `source` (mail formats need mail,
    /// iCalendar needs calendars, vCard address books).
    pub fn accepts(self, source: &ExportSource) -> bool {
        match source {
            ExportSource::Folders { .. } | ExportSource::Messages { .. } => {
                matches!(self, Self::Mbox | Self::Eml | Self::Maildir)
            }
            ExportSource::Calendars { .. } => self == Self::Ics,
            ExportSource::AddressBooks { .. } => self == Self::Vcard,
        }
    }

    /// Whether the export target is a folder (else one file, or one file
    /// per folder / collection inside the target folder when several are
    /// exported).
    pub fn is_directory(self) -> bool {
        matches!(self, Self::Eml | Self::Maildir)
    }

    /// The file extension written, if the format writes files with one.
    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Mbox => Some("mbox"),
            Self::Eml => Some("eml"),
            Self::Maildir => None,
            Self::Ics => Some("ics"),
            Self::Vcard => Some("vcf"),
        }
    }
}

labeled_enum! {
    /// The mbox dialect an export writes (`mbox-variant-*`).
    #[derive(Default)]
    pub enum MboxVariant : "mbox-variant" {
        /// `>From ` quoting reversible on read (the default).
        #[default]
        Mboxrd = "mboxrd",
        /// Only `From ` lines quoted (older readers).
        Mboxo = "mboxo",
    }
}

/// A source to import from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ImportSource {
    /// What it is.
    pub format: ImportFormat,
    /// Absolute path of the file or folder.
    pub path: String,
}

/// A source the core found on this computer
/// ([`crate::Request::FindImportSources`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FoundImportSource {
    /// The source.
    pub source: ImportSource,
    /// A label (the Thunderbird profile's name).
    pub label: String,
    /// Whether it is the program's default profile.
    #[serde(default)]
    pub is_default: bool,
    /// When it was last used (RFC 3339), if known.
    #[serde(default)]
    pub last_used_at: Option<String>,
}

/// One mail folder of an import source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ImportFolderPreview {
    /// Its path inside the source (`/`-separated, e.g. `Local
    /// Folders/Archives/2024`); the key [`ImportOptions::folders`] names.
    pub path: String,
    /// Display name (the last path segment, decoded).
    pub name: String,
    /// Messages found (an estimate for mbox files above 64 MiB until the
    /// import reads them).
    pub messages: u64,
    /// Bytes of mail.
    pub size_bytes: u64,
}

labeled_enum! {
    /// What a calendar / address book of a source holds
    /// (`import-collection-*`).
    pub enum ImportCollectionKind : "import-collection" {
        /// Events (and possibly tasks).
        Calendar = "calendar",
        /// Tasks only.
        TaskList = "task-list",
        /// Contacts and groups.
        AddressBook = "address-book",
    }
}

/// A calendar, task list or address book of an import source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ImportCollectionPreview {
    /// Key inside the source (file path or Thunderbird book URI); the key
    /// [`ImportOptions::collections`] names.
    pub key: String,
    /// Display name.
    pub name: String,
    /// What it holds.
    pub kind: ImportCollectionKind,
    /// Items found.
    pub items: u64,
}

/// An account found in another program, as Nail would configure it
/// (secrets are never imported: the user signs in or types the password
/// when adding it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountProposal {
    /// Key in the source (Thunderbird `account3`).
    pub key: String,
    /// The proposed configuration (`id` empty: the core derives one on
    /// `AddAccount`).
    pub config: AccountConfig,
    /// Whether an account with the same login already exists in Nail.
    #[serde(default)]
    pub duplicate_of: Option<AccountId>,
    /// What could not be carried over.
    #[serde(default)]
    pub issues: Vec<ItemIssue>,
}

/// A filter of another program as a Nail rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RuleProposal {
    /// Key in the source (Thunderbird: account key + filter index).
    pub key: String,
    /// The account the filter belonged to (its proposal key).
    #[serde(default)]
    pub account_key: Option<String>,
    /// The rule (`id` empty; disabled when the original was).
    pub rule: Rule,
    /// Whether every condition and action could be expressed; `false` =
    /// parts were dropped (see `issues`) and the rule starts disabled.
    pub complete: bool,
    /// What could not be expressed.
    #[serde(default)]
    pub issues: Vec<ItemIssue>,
}

/// What an import would bring in ([`crate::Request::PreviewImport`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ImportPreview {
    /// The source read.
    pub source: ImportSource,
    /// Mail folders (empty for `.ics` / `.vcf`).
    #[serde(default)]
    pub folders: Vec<ImportFolderPreview>,
    /// Calendars, task lists and address books.
    #[serde(default)]
    pub collections: Vec<ImportCollectionPreview>,
    /// Totals.
    pub counts: JobCounts,
    /// Thunderbird accounts.
    #[serde(default)]
    pub accounts: Vec<AccountProposal>,
    /// Thunderbird filters.
    #[serde(default)]
    pub rules: Vec<RuleProposal>,
    /// The first problems found (at most [`MAX_PREVIEW_ISSUES`]).
    #[serde(default)]
    pub issues: Vec<ItemIssue>,
    /// Problems not listed.
    #[serde(default)]
    pub issues_truncated: u64,
}

/// Where imported mail goes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct MailImportTarget {
    /// The account; `null` = "Local Folders" ([`LOCAL_FOLDERS_ACCOUNT_ID`]).
    pub account_id: Option<AccountId>,
    /// The folder the source's tree goes under; `null` = the account's
    /// top level.
    pub parent_folder_id: Option<FolderId>,
    /// A folder created for the import under the parent (e.g. "Thunderbird
    /// import"); `null` = the source's folders go directly under the parent
    /// (same-named folders are merged).
    pub folder_name: Option<String>,
}

/// How to import ([`crate::Request::StartImport`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ImportOptions {
    /// Where mail goes.
    pub mail: MailImportTarget,
    /// The folders to import (paths from the preview); `null` = all.
    pub folders: Option<Vec<String>>,
    /// The collections to import (keys from the preview); `null` = all.
    pub collections: Option<Vec<String>>,
    /// Target for events and tasks; `null` = the default calendar.
    pub calendar_id: Option<CalendarId>,
    /// Target for contacts; `null` = the default address book.
    pub address_book_id: Option<AddressBookId>,
    /// Skip messages already in the target folder (same `Message-ID`,
    /// date and size) — so an interrupted import can simply be repeated.
    pub skip_duplicates: bool,
    /// Keep read / flagged / answered / draft flags and keywords found in
    /// the source (Maildir info, mbox `Status` / `X-Status` /
    /// `X-Mozilla-Status`).
    pub keep_flags: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            mail: MailImportTarget::default(),
            folders: None,
            collections: None,
            calendar_id: None,
            address_book_id: None,
            skip_duplicates: true,
            keep_flags: true,
        }
    }
}

/// What to export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ExportSource {
    /// Mail folders.
    Folders {
        /// The folders.
        folder_ids: Vec<FolderId>,
        /// Their subfolders too (as subfolders / further mbox files).
        #[serde(default)]
        include_subfolders: bool,
    },
    /// Chosen messages (one mbox file / one folder of `.eml` files).
    Messages {
        /// The messages.
        message_ids: Vec<MessageId>,
    },
    /// Calendars and task lists (iCalendar).
    Calendars {
        /// The calendars.
        calendar_ids: Vec<CalendarId>,
    },
    /// Address books (vCard).
    AddressBooks {
        /// The address books.
        address_book_ids: Vec<AddressBookId>,
    },
}

impl ExportSource {
    /// Whether it names nothing.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Folders { folder_ids, .. } => folder_ids.is_empty(),
            Self::Messages { message_ids } => message_ids.is_empty(),
            Self::Calendars { calendar_ids } => calendar_ids.is_empty(),
            Self::AddressBooks { address_book_ids } => address_book_ids.is_empty(),
        }
    }
}

/// How to export ([`crate::Request::StartExport`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ExportOptions {
    /// Replace existing files; `false` = `conflict` when the target exists
    /// (a folder target must be empty or new).
    pub overwrite: bool,
    /// The mbox dialect.
    pub mbox_variant: MboxVariant,
    /// Write messages only stored as headers (headers-only folders) as
    /// they are; `false` = fetch their bodies first (needs the server).
    pub allow_headers_only: bool,
}

labeled_enum! {
    /// What a job does (`portability-job-*`).
    pub enum JobKind : "portability-job" {
        /// An import.
        Import = "import",
        /// An export.
        Export = "export",
        /// Writing a backup archive.
        Backup = "backup",
        /// Reading a backup archive back and checking every entry.
        VerifyBackup = "verify-backup",
        /// Restoring from a backup archive.
        Restore = "restore",
    }
}

labeled_enum! {
    /// Where a job is (`portability-phase-*`).
    pub enum JobPhase : "portability-phase" {
        /// Waiting for another job of the same target to finish.
        Queued = "queued",
        /// Counting what there is.
        Scanning = "scanning",
        /// Reading the source.
        Reading = "reading",
        /// Writing the target.
        Writing = "writing",
        /// Checking what was written (backups, exports).
        Verifying = "verifying",
        /// Cleaning up, writing the report.
        Finishing = "finishing",
    }
}

labeled_enum! {
    /// How a job stands (`portability-state-*`).
    pub enum JobState : "portability-state" {
        /// Running (or queued).
        Running = "running",
        /// Finished without problems.
        Completed = "completed",
        /// Finished; some items had problems (see the report's issues).
        CompletedWithIssues = "completed-with-issues",
        /// Stopped by an error (the report's `error`).
        Failed = "failed",
        /// Cancelled by the user (what was written stays; see the report).
        Cancelled = "cancelled",
    }
}

impl JobState {
    /// Whether the job is over.
    pub fn is_finished(self) -> bool {
        self != Self::Running
    }
}

/// Progress of a running job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct JobProgress {
    /// The phase.
    pub phase: JobPhase,
    /// Items done (messages, events, contacts, archive entries).
    pub done: u64,
    /// Items in total, `null` while unknown (still scanning).
    #[serde(default)]
    pub total: Option<u64>,
    /// Bytes read or written so far.
    #[serde(default)]
    pub bytes_done: u64,
    /// Bytes in total, if known.
    #[serde(default)]
    pub bytes_total: Option<u64>,
    /// What is being worked on (a folder name), for display.
    #[serde(default)]
    pub current: Option<String>,
}

impl JobProgress {
    /// A job that has not started.
    pub fn queued() -> Self {
        Self {
            phase: JobPhase::Queued,
            done: 0,
            total: None,
            bytes_done: 0,
            bytes_total: None,
            current: None,
        }
    }

    /// Done as a fraction 0..=1 (bytes when known, else items), `None`
    /// while the total is unknown.
    pub fn fraction(&self) -> Option<f64> {
        let (done, total) = match (self.bytes_total, self.total) {
            (Some(total), _) if total > 0 => (self.bytes_done, total),
            (_, Some(total)) if total > 0 => (self.done, total),
            (_, Some(0)) | (Some(0), None) => return Some(1.0),
            _ => return None,
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a progress fraction; counts far below 2^52 are exact"
        )]
        Some((done.min(total) as f64) / (total as f64))
    }
}

/// What a job (or preview) counted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct JobCounts {
    /// Messages imported / exported / backed up / restored.
    pub messages: u64,
    /// Folders created or written.
    pub folders: u64,
    /// Events.
    pub events: u64,
    /// Tasks.
    pub tasks: u64,
    /// Contacts.
    pub contacts: u64,
    /// Contact groups.
    pub groups: u64,
    /// Account proposals / restored account configurations.
    pub accounts: u64,
    /// Rule proposals / restored rules.
    pub rules: u64,
    /// Other files (settings, theme, layout, plug-ins, keys).
    pub files: u64,
    /// Items left out because they were already there.
    pub duplicates: u64,
    /// Items left out because of a problem (each has an issue, up to
    /// [`MAX_JOB_ISSUES`]).
    pub skipped: u64,
}

labeled_enum! {
    /// How bad an item issue is (`portability-severity-*`).
    pub enum IssueSeverity : "portability-severity" {
        /// The item was imported / written, but changed (e.g. an unknown
        /// flag dropped, a folder renamed).
        Warning = "warning",
        /// The item was left out.
        Skipped = "skipped",
    }
}

labeled_enum! {
    /// What went wrong with one item (`portability-issue-*`).
    pub enum PortabilityIssueCode : "portability-issue" {
        /// A file could not be read (permissions, locked by another
        /// program, I/O error).
        UnreadableFile = "unreadable-file",
        /// Not a valid message (no headers, broken MIME structure).
        MalformedMessage = "malformed-message",
        /// Larger than [`MAX_IMPORT_MESSAGE_BYTES`].
        TooLarge = "too-large",
        /// The last message of an mbox file is cut off.
        Truncated = "truncated",
        /// Already in the target (skipped by `skip_duplicates`).
        Duplicate = "duplicate",
        /// A flag / keyword Nail does not know (dropped).
        UnknownFlag = "unknown-flag",
        /// A folder name that is not allowed (renamed).
        InvalidFolderName = "invalid-folder-name",
        /// Calendar data that could not be read.
        InvalidCalendarData = "invalid-calendar-data",
        /// Contact data that could not be read.
        InvalidContactData = "invalid-contact-data",
        /// A format variant Nail cannot read (e.g. a Mork address book it
        /// cannot parse, an old Thunderbird version).
        UnsupportedFormat = "unsupported-format",
        /// A Thunderbird filter condition or action without a Nail
        /// equivalent (dropped from the proposal).
        FilterNotExpressible = "filter-not-expressible",
        /// An account setting without a Nail equivalent (dropped from the
        /// proposal).
        AccountNotExpressible = "account-not-expressible",
        /// A password or token found in the source was not imported
        /// (never are; the user signs in again).
        SecretNotImported = "secret-not-imported",
        /// Writing the item failed (disk full, target folder gone).
        WriteFailed = "write-failed",
        /// A target file existed and was not overwritten.
        TargetExists = "target-exists",
        /// A backup entry's checksum does not match (corrupt archive).
        ChecksumMismatch = "checksum-mismatch",
        /// An entry the backup manifest lists is missing.
        EntryMissing = "entry-missing",
        /// The backup was made by a newer Nail (entry format unknown).
        NewerFormat = "newer-format",
        /// Restoring this item needs the user's attention (an account's
        /// password, a plug-in's grants).
        NeedsAttention = "needs-attention",
        /// Deleted in the source program but still in its files (a
        /// Thunderbird folder not compacted yet, a Maildir message marked
        /// `T`): not imported (WP125).
        DeletedInSource = "deleted-in-source",
        /// The folder does not exist in the target account; its messages
        /// went into the chosen folder (imports into an account create no
        /// folders on the server; WP125, ADR 0099).
        FolderMerged = "folder-merged",
    }
}

/// Where in a source or target an issue is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ItemRef {
    /// The file (absolute, or relative to the source / archive).
    pub file: Option<String>,
    /// The folder / collection (source path or Nail name).
    pub folder: Option<String>,
    /// 1-based position in the file / folder (mbox: the n-th message).
    pub index: Option<u64>,
    /// Byte offset in the file (mbox).
    pub offset: Option<u64>,
    /// The item as the user knows it: subject, event title, contact name,
    /// filter name, archive entry name.
    pub label: Option<String>,
}

/// A problem with one item of a job or preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ItemIssue {
    /// Where.
    pub item: ItemRef,
    /// Whether the item was left out.
    pub severity: IssueSeverity,
    /// What.
    pub code: PortabilityIssueCode,
    /// Diagnostic detail (English, for logs and the report's details).
    pub detail: String,
    /// The user-facing text: [`PortabilityIssueCode::message_id`] by
    /// default, a more specific `core-portability.ftl` id when there is one.
    pub localized: LocalizedMessage,
}

impl ItemIssue {
    /// An issue with the code's own text.
    pub fn new(
        code: PortabilityIssueCode,
        severity: IssueSeverity,
        item: ItemRef,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            item,
            severity,
            code,
            detail: detail.into(),
            localized: LocalizedMessage::new(code.message_id()),
        }
    }
}

/// What a finished job did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct JobReport {
    /// The job.
    pub job_id: JobId,
    /// What it did.
    pub kind: JobKind,
    /// How it ended.
    pub state: JobState,
    /// When it started (RFC 3339).
    pub started_at: String,
    /// When it ended (RFC 3339).
    pub finished_at: String,
    /// The source path (import, verify, restore).
    #[serde(default)]
    pub source: Option<String>,
    /// The target path (export, backup).
    #[serde(default)]
    pub target: Option<String>,
    /// What was done.
    pub counts: JobCounts,
    /// Item problems (at most [`MAX_JOB_ISSUES`]).
    #[serde(default)]
    pub issues: Vec<ItemIssue>,
    /// Problems not listed.
    #[serde(default)]
    pub issues_truncated: u64,
    /// Folders an import created (to open them).
    #[serde(default)]
    pub created_folders: Vec<FolderId>,
    /// Thunderbird account proposals (import).
    #[serde(default)]
    pub accounts: Vec<AccountProposal>,
    /// Thunderbird filter proposals (import).
    #[serde(default)]
    pub rules: Vec<RuleProposal>,
    /// The archive's manifest (backup, verify, restore).
    #[serde(default)]
    pub backup: Option<BackupManifest>,
    /// What a restore did.
    #[serde(default)]
    pub restore: Option<RestoreReport>,
    /// Error class when `failed`.
    #[serde(default)]
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English) when `failed`.
    #[serde(default)]
    pub message: Option<String>,
    /// The user-facing text when `failed` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

impl JobReport {
    /// An empty report of a job that is starting (state `running`).
    pub fn started(job_id: JobId, kind: JobKind, started_at: impl Into<String>) -> Self {
        let started_at = started_at.into();
        Self {
            job_id,
            kind,
            state: JobState::Running,
            finished_at: started_at.clone(),
            started_at,
            source: None,
            target: None,
            counts: JobCounts::default(),
            issues: Vec::new(),
            issues_truncated: 0,
            created_folders: Vec::new(),
            accounts: Vec::new(),
            rules: Vec::new(),
            backup: None,
            restore: None,
            error: None,
            message: None,
            localized: None,
        }
    }

    /// Records an issue (kept up to [`MAX_JOB_ISSUES`], then counted);
    /// `skipped` issues also count as skipped items.
    pub fn push_issue(&mut self, issue: ItemIssue) {
        if issue.severity == IssueSeverity::Skipped {
            if issue.code == PortabilityIssueCode::Duplicate {
                self.counts.duplicates += 1;
            } else {
                self.counts.skipped += 1;
            }
        }
        if self.issues.len() < MAX_JOB_ISSUES {
            self.issues.push(issue);
        } else {
            self.issues_truncated += 1;
        }
    }

    /// The end state for a job that ran to the end: `completed`, or
    /// `completed-with-issues` when anything was skipped or warned about
    /// (duplicates alone do not count).
    pub fn finished_state(&self) -> JobState {
        let problems = self.issues_truncated > 0
            || self
                .issues
                .iter()
                .any(|i| i.code != PortabilityIssueCode::Duplicate);
        if problems {
            JobState::CompletedWithIssues
        } else {
            JobState::Completed
        }
    }
}

/// A job as listed ([`crate::Request::ListPortabilityJobs`],
/// [`crate::Request::GetPortabilityJob`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct JobStatus {
    /// The job.
    pub job_id: JobId,
    /// What it does.
    pub kind: JobKind,
    /// How it stands.
    pub state: JobState,
    /// When it started (RFC 3339).
    pub started_at: String,
    /// The source or target path, for display.
    pub path: String,
    /// The latest progress.
    pub progress: JobProgress,
    /// The report, once finished.
    #[serde(default)]
    pub report: Option<JobReport>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn labels_match_the_wire() {
        assert_labels(
            ImportFormat::ALL,
            ImportFormat::as_str,
            ImportFormat::message_id,
        );
        assert_labels(
            ExportFormat::ALL,
            ExportFormat::as_str,
            ExportFormat::message_id,
        );
        assert_labels(
            MboxVariant::ALL,
            MboxVariant::as_str,
            MboxVariant::message_id,
        );
        assert_labels(
            ImportCollectionKind::ALL,
            ImportCollectionKind::as_str,
            ImportCollectionKind::message_id,
        );
        assert_labels(JobKind::ALL, JobKind::as_str, JobKind::message_id);
        assert_labels(JobPhase::ALL, JobPhase::as_str, JobPhase::message_id);
        assert_labels(JobState::ALL, JobState::as_str, JobState::message_id);
        assert_labels(
            IssueSeverity::ALL,
            IssueSeverity::as_str,
            IssueSeverity::message_id,
        );
        assert_labels(
            PortabilityIssueCode::ALL,
            PortabilityIssueCode::as_str,
            PortabilityIssueCode::message_id,
        );
        assert_eq!(
            PortabilityIssueCode::FilterNotExpressible.message_id(),
            "portability-issue-filter-not-expressible"
        );
    }

    #[test]
    fn formats_know_their_sources() {
        assert!(ImportFormat::Maildir.is_directory());
        assert!(!ImportFormat::Mbox.is_directory());
        assert!(ImportFormat::ThunderbirdProfile.has_mail());
        assert!(ImportFormat::ThunderbirdProfile.has_pim());
        assert!(!ImportFormat::Ics.has_mail());
        let folders = ExportSource::Folders {
            folder_ids: vec!["f".into()],
            include_subfolders: true,
        };
        let books = ExportSource::AddressBooks {
            address_book_ids: vec!["b".into()],
        };
        assert!(ExportFormat::Mbox.accepts(&folders));
        assert!(!ExportFormat::Ics.accepts(&folders));
        assert!(ExportFormat::Vcard.accepts(&books));
        assert!(!ExportFormat::Maildir.accepts(&books));
        assert!(ExportFormat::Maildir.is_directory());
        assert_eq!(ExportFormat::Vcard.extension(), Some("vcf"));
        assert!(
            ExportSource::Messages {
                message_ids: vec![]
            }
            .is_empty()
        );
    }

    #[test]
    fn wire_shapes() {
        let source = ImportSource {
            format: ImportFormat::ThunderbirdProfile,
            path: r"C:\Users\a\AppData\Roaming\Thunderbird\Profiles\x.default".into(),
        };
        assert_eq!(
            serde_json::to_value(&source).unwrap(),
            json!({"format": "thunderbird-profile",
                   "path": r"C:\Users\a\AppData\Roaming\Thunderbird\Profiles\x.default"})
        );
        assert_eq!(
            serde_json::to_value(ExportSource::Calendars {
                calendar_ids: vec!["c1".into()]
            })
            .unwrap(),
            json!({"type": "calendars", "calendar_ids": ["c1"]})
        );
        // Options default when omitted (the UI may send `{}`).
        let options: ImportOptions = serde_json::from_value(json!({})).unwrap();
        assert_eq!(options, ImportOptions::default());
        assert!(options.skip_duplicates && options.keep_flags);
        let export: ExportOptions = serde_json::from_value(json!({})).unwrap();
        assert_eq!(export.mbox_variant, MboxVariant::Mboxrd);
    }

    #[test]
    fn progress_fraction_prefers_bytes() {
        let mut p = JobProgress::queued();
        assert_eq!(p.fraction(), None);
        p.total = Some(4);
        p.done = 1;
        assert_eq!(p.fraction(), Some(0.25));
        p.bytes_total = Some(1000);
        p.bytes_done = 500;
        assert_eq!(p.fraction(), Some(0.5));
        p.bytes_done = 5000;
        assert_eq!(p.fraction(), Some(1.0));
        let empty = JobProgress {
            total: Some(0),
            ..JobProgress::queued()
        };
        assert_eq!(empty.fraction(), Some(1.0));
    }

    #[test]
    fn reports_cap_issues_and_count_skips() {
        let mut report = JobReport::started("j1".into(), JobKind::Import, "2026-10-02T10:00:00Z");
        assert_eq!(report.finished_state(), JobState::Completed);
        report.push_issue(ItemIssue::new(
            PortabilityIssueCode::Duplicate,
            IssueSeverity::Skipped,
            ItemRef::default(),
            "dup",
        ));
        assert_eq!(report.counts.duplicates, 1);
        assert_eq!(report.finished_state(), JobState::Completed);
        for n in 0..(MAX_JOB_ISSUES + 5) {
            report.push_issue(ItemIssue::new(
                PortabilityIssueCode::MalformedMessage,
                IssueSeverity::Skipped,
                ItemRef {
                    index: Some(n as u64),
                    ..ItemRef::default()
                },
                "bad",
            ));
        }
        assert_eq!(report.issues.len(), MAX_JOB_ISSUES);
        assert_eq!(report.issues_truncated, 6);
        assert_eq!(report.counts.skipped, (MAX_JOB_ISSUES + 5) as u64);
        assert_eq!(report.finished_state(), JobState::CompletedWithIssues);
        assert_eq!(
            report.issues[1].localized.id,
            "portability-issue-malformed-message"
        );
        assert!(JobState::Cancelled.is_finished());
        assert!(!JobState::Running.is_finished());
    }
}
