//! Gmail accounts through the Gmail API (M3b, ADR 0065).
//!
//! A Gmail account (`gmail` node in `accounts.kdl`,
//! [`crate::accounts::IncomingServer::Gmail`]) syncs with the Gmail REST API
//! instead of IMAP: `users.history.list` for incremental changes (polled
//! with an adaptive interval; push needs Cloud Pub/Sub, which a desktop
//! client cannot receive), `users.messages.get?format=raw` for MIME,
//! `users.messages.modify` / `trash` / `send`, `users.labels`.
//!
//! # How labels map to Nail
//!
//! A Gmail message is stored **once** (Gmail ids are per message, not per
//! label). Its *folder* follows from its labels ([`GmailFolder::for_labels`]):
//! `TRASH` → Trash, `SPAM` → Junk, `DRAFT` → Drafts, `INBOX` → Inbox,
//! `SENT` → Sent, anything else → the Archive folder ("All Mail"). Moving a
//! message between these folders changes labels
//! ([`GmailFolder::move_change`]).
//!
//! * `UNREAD` ↔ not `\Seen`, `STARRED` ↔ `\Flagged`, `DRAFT` ↔ `\Draft`.
//! * **User labels** ↔ Nail **tags**: the tag is the label's name
//!   (`Work/Projects`), so rules, search (`tag:`) and the tag UI work
//!   unchanged; the store maps label ids ↔ names (`gmail_labels`), renaming
//!   a label renames its tag, and a new tag on a Gmail message creates the
//!   label on the server.
//! * **Categories** (`CATEGORY_PERSONAL`, `…_SOCIAL`, `…_PROMOTIONS`,
//!   `…_UPDATES`, `…_FORUMS`) become [`GmailCategory`] on
//!   [`GmailMessageInfo`], a filterable property (query `category:`).
//! * Gmail **thread ids** are used directly as Nail thread ids for Gmail
//!   messages (no JWZ guessing; `ThreadId` = `gmail-<account>-<threadId>`).
//! * Other system labels (`IMPORTANT`, `CHAT`, `YELLOW_STAR`, …) stay in
//!   [`GmailMessageInfo::label_ids`] only.
//!
//! Label management ([`crate::Request::CreateGmailLabel`] …) goes through
//! a per-account label operation queue in the store (`gmail_label_ops`),
//! like every other write.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::FolderRole;
use crate::ids::{AccountId, FolderId, GmailLabelId};
use crate::ops::FlagChange;

/// The Gmail API host (the default `host` of a `gmail` node).
pub const GMAIL_API_HOST: &str = "gmail.googleapis.com";

/// The API base path below the host (`https://<host>/gmail/v1/users/me/…`).
pub const GMAIL_API_BASE_PATH: &str = "/gmail/v1/users/me";

/// The OAuth2 scope Nail asks Google for: full mail access (restricted
/// scope; the same one IMAP needs, so the built-in `google` provider
/// already requests it).
pub const GMAIL_SCOPE: &str = "https://mail.google.com/";

/// Longest label name Gmail accepts (characters).
pub const MAX_LABEL_NAME: usize = 225;

/// System label ids Nail interprets.
pub mod system {
    /// The inbox.
    pub const INBOX: &str = "INBOX";
    /// Sent mail.
    pub const SENT: &str = "SENT";
    /// Drafts.
    pub const DRAFT: &str = "DRAFT";
    /// Spam.
    pub const SPAM: &str = "SPAM";
    /// Trash.
    pub const TRASH: &str = "TRASH";
    /// Unread (absent = `\Seen`).
    pub const UNREAD: &str = "UNREAD";
    /// Starred (= `\Flagged`).
    pub const STARRED: &str = "STARRED";
    /// Marked important.
    pub const IMPORTANT: &str = "IMPORTANT";
    /// Chat transcripts.
    pub const CHAT: &str = "CHAT";
    /// Category "Primary".
    pub const CATEGORY_PERSONAL: &str = "CATEGORY_PERSONAL";
    /// Category "Social".
    pub const CATEGORY_SOCIAL: &str = "CATEGORY_SOCIAL";
    /// Category "Promotions".
    pub const CATEGORY_PROMOTIONS: &str = "CATEGORY_PROMOTIONS";
    /// Category "Updates".
    pub const CATEGORY_UPDATES: &str = "CATEGORY_UPDATES";
    /// Category "Forums".
    pub const CATEGORY_FORUMS: &str = "CATEGORY_FORUMS";

    /// Every system label id above (user labels cannot take these names).
    pub const ALL: [&str; 14] = [
        INBOX,
        SENT,
        DRAFT,
        SPAM,
        TRASH,
        UNREAD,
        STARRED,
        IMPORTANT,
        CHAT,
        CATEGORY_PERSONAL,
        CATEGORY_SOCIAL,
        CATEGORY_PROMOTIONS,
        CATEGORY_UPDATES,
        CATEGORY_FORUMS,
    ];
}

/// Account settings of a `gmail` node (`accounts.kdl`). `null` = the
/// documented default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct GmailOptions {
    /// Shortest polling interval in seconds (right after a change or a
    /// user action). Default 30, at least 10.
    pub poll_interval_secs: Option<u32>,
    /// Longest polling interval in seconds (after a quiet period the
    /// interval doubles up to this). Default 300.
    pub max_poll_interval_secs: Option<u32>,
}

impl GmailOptions {
    /// The effective (minimum, maximum) polling interval in seconds; the
    /// maximum is never below the minimum.
    pub fn poll_range(&self) -> (u32, u32) {
        crate::engine::AdaptivePoll::range(self.poll_interval_secs, self.max_poll_interval_secs)
    }
}

/// The folders of a Gmail account (system labels shown as folders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailFolder {
    /// `INBOX`.
    Inbox,
    /// `SENT` (and not in the inbox).
    Sent,
    /// `DRAFT`.
    Drafts,
    /// `SPAM`.
    Spam,
    /// `TRASH`.
    Trash,
    /// Everything else ("All Mail" without inbox / sent / spam / trash).
    Archive,
}

impl GmailFolder {
    /// Every folder, in display order.
    pub const ALL: [GmailFolder; 6] = [
        Self::Inbox,
        Self::Sent,
        Self::Drafts,
        Self::Archive,
        Self::Spam,
        Self::Trash,
    ];

    /// The folder's path in the store (stable, not shown).
    pub fn path(self) -> &'static str {
        match self {
            Self::Inbox => "INBOX",
            Self::Sent => "SENT",
            Self::Drafts => "DRAFT",
            Self::Spam => "SPAM",
            Self::Trash => "TRASH",
            Self::Archive => "ARCHIVE",
        }
    }

    /// The folder for a store path, if it is one of these.
    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.path() == path)
    }

    /// The folder's special-use role.
    pub fn role(self) -> FolderRole {
        match self {
            Self::Inbox => FolderRole::Inbox,
            Self::Sent => FolderRole::Sent,
            Self::Drafts => FolderRole::Drafts,
            Self::Spam => FolderRole::Junk,
            Self::Trash => FolderRole::Trash,
            Self::Archive => FolderRole::Archive,
        }
    }

    /// The system label that puts a message here (`None` for Archive,
    /// which is the absence of the others).
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Inbox => Some(system::INBOX),
            Self::Sent => Some(system::SENT),
            Self::Drafts => Some(system::DRAFT),
            Self::Spam => Some(system::SPAM),
            Self::Trash => Some(system::TRASH),
            Self::Archive => None,
        }
    }

    /// The folder a message with these labels lives in. Precedence:
    /// Trash, Spam, Drafts, Inbox, Sent, Archive.
    pub fn for_labels<S: AsRef<str>>(label_ids: &[S]) -> Self {
        let has = |l: &str| label_ids.iter().any(|x| x.as_ref() == l);
        [
            Self::Trash,
            Self::Spam,
            Self::Drafts,
            Self::Inbox,
            Self::Sent,
        ]
        .into_iter()
        .find(|f| f.label().is_some_and(has))
        .unwrap_or(Self::Archive)
    }

    /// The label change that moves a message from `self` to `to`.
    /// Moving into Trash is `users.messages.trash` on the server (the
    /// change adds `TRASH`); out of Trash or Spam removes those; into the
    /// Archive removes `INBOX` (and `SPAM` / `TRASH`). Trash / Spam → Sent
    /// only removes `TRASH` / `SPAM` (the message keeps its `SENT` label:
    /// the undo of deleting the user's replies). `None` where Gmail has no
    /// such move: Sent from elsewhere and Drafts as targets (their labels
    /// follow from sending / drafting), drafts as a source (the drafts API
    /// owns them) and Sent → Archive (sent mail stays in Sent).
    pub fn move_change(self, to: GmailFolder) -> Option<LabelChange> {
        if self == to {
            return Some(LabelChange::default());
        }
        if to == Self::Drafts
            || (to == Self::Sent && !matches!(self, Self::Trash | Self::Spam))
            || self == Self::Drafts
            || (self == Self::Sent && to == Self::Archive)
        {
            return None;
        }
        let mut change = LabelChange::default();
        // Leaving Trash / Spam removes their label; leaving the inbox for
        // another folder removes INBOX.
        for (folder, label) in [
            (Self::Trash, system::TRASH),
            (Self::Spam, system::SPAM),
            (Self::Inbox, system::INBOX),
        ] {
            if self == folder && to != folder {
                change.remove.push(label.to_owned());
            }
        }
        match to {
            Self::Inbox => {
                change.add.push(system::INBOX.to_owned());
                change.remove_all(&[system::SPAM, system::TRASH]);
            }
            Self::Spam => {
                change.add.push(system::SPAM.to_owned());
                change.remove_all(&[system::INBOX]);
            }
            Self::Trash => change.add.push(system::TRASH.to_owned()),
            Self::Archive => change.remove_all(&[system::INBOX, system::SPAM, system::TRASH]),
            Self::Sent | Self::Drafts => {}
        }
        Some(change)
    }
}

/// Labels to add and remove (`users.messages.modify`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct LabelChange {
    /// Label ids to add.
    pub add: Vec<String>,
    /// Label ids to remove.
    pub remove: Vec<String>,
}

impl LabelChange {
    /// Adds each of `labels` to [`Self::remove`] unless it is there already.
    fn remove_all(&mut self, labels: &[&str]) {
        for &label in labels {
            if !self.remove.iter().any(|l| l == label) {
                self.remove.push(label.to_owned());
            }
        }
    }
}

impl LabelChange {
    /// Whether nothing changes.
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }

    /// The system-label part of a Nail flag change: `\Seen` ↔ `UNREAD`,
    /// `\Flagged` ↔ `STARRED` (`\Answered` has no Gmail label; `\Draft` is
    /// not changed by flags). Tags are mapped to user labels by the engine
    /// (it needs the account's label table).
    pub fn from_flags(change: &FlagChange) -> Self {
        let mut out = Self::default();
        match change.seen {
            Some(true) => out.remove.push(system::UNREAD.to_owned()),
            Some(false) => out.add.push(system::UNREAD.to_owned()),
            None => {}
        }
        match change.flagged {
            Some(true) => out.add.push(system::STARRED.to_owned()),
            Some(false) => out.remove.push(system::STARRED.to_owned()),
            None => {}
        }
        out
    }
}

/// A Gmail inbox category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailCategory {
    /// `CATEGORY_PERSONAL` ("Primary").
    Primary,
    /// `CATEGORY_SOCIAL`.
    Social,
    /// `CATEGORY_PROMOTIONS`.
    Promotions,
    /// `CATEGORY_UPDATES`.
    Updates,
    /// `CATEGORY_FORUMS`.
    Forums,
}

impl GmailCategory {
    /// Every category, in Gmail's tab order.
    pub const ALL: [GmailCategory; 5] = [
        Self::Primary,
        Self::Social,
        Self::Promotions,
        Self::Updates,
        Self::Forums,
    ];

    /// The category's label id.
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => system::CATEGORY_PERSONAL,
            Self::Social => system::CATEGORY_SOCIAL,
            Self::Promotions => system::CATEGORY_PROMOTIONS,
            Self::Updates => system::CATEGORY_UPDATES,
            Self::Forums => system::CATEGORY_FORUMS,
        }
    }

    /// The wire / query name (`primary`, `social`, …), as used by the
    /// query language's `category:`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Social => "social",
            Self::Promotions => "promotions",
            Self::Updates => "updates",
            Self::Forums => "forums",
        }
    }

    /// The category of a query name (case-insensitive).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(name))
    }

    /// The category among a message's labels (the first one Gmail lists
    /// in tab order; Gmail sets at most one).
    pub fn for_labels<S: AsRef<str>>(label_ids: &[S]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| label_ids.iter().any(|l| l.as_ref() == c.label()))
    }

    /// The Fluent id of the category's name (`gmail-category-<name>`).
    pub fn message_id(self) -> String {
        format!("gmail-category-{}", self.as_str())
    }
}

/// Gmail-specific data of a message ([`crate::MessageSummary::gmail`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GmailMessageInfo {
    /// Gmail's message id.
    pub gmail_id: String,
    /// Gmail's thread id.
    pub thread_id: String,
    /// Every label id on the message (system and user labels).
    pub label_ids: Vec<String>,
    /// The inbox category, if any.
    #[serde(default)]
    pub category: Option<GmailCategory>,
}

/// Whether a label is Gmail's own or the user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailLabelKind {
    /// A system label (`INBOX`, `CATEGORY_SOCIAL`, …): cannot be renamed
    /// or deleted.
    System,
    /// A user label.
    User,
}

/// Whether a label shows in the label list (`labelListVisibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailLabelListVisibility {
    /// `labelShow` (the default).
    #[default]
    Show,
    /// `labelShowIfUnread`.
    ShowIfUnread,
    /// `labelHide`.
    Hide,
}

/// Whether a label shows on messages (`messageListVisibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailMessageListVisibility {
    /// `show` (the default).
    #[default]
    Show,
    /// `hide`.
    Hide,
}

/// A label colour (`#rrggbb` each). Gmail accepts only colours of its own
/// palette; others are refused by the server (`rejected`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct GmailLabelColor {
    /// Background colour.
    pub background: String,
    /// Text colour.
    pub text: String,
}

/// The editable settings of a user label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GmailLabelSettings {
    /// Name; `/` nests (`Work/Projects`). Also the Nail tag.
    pub name: String,
    /// Colour, `null` = none.
    #[serde(default)]
    pub color: Option<GmailLabelColor>,
    /// Visibility in the label list.
    #[serde(default)]
    pub list_visibility: GmailLabelListVisibility,
    /// Visibility on messages.
    #[serde(default)]
    pub message_visibility: GmailMessageListVisibility,
}

impl GmailLabelSettings {
    /// Settings with a name and defaults for the rest.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: None,
            list_visibility: GmailLabelListVisibility::Show,
            message_visibility: GmailMessageListVisibility::Show,
        }
    }

    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<GmailLabelIssue> {
        let mut out = Vec::new();
        let name = self.name.trim();
        if name.is_empty() {
            out.push(GmailLabelIssue::MissingName);
        } else {
            if name.chars().count() > MAX_LABEL_NAME {
                out.push(GmailLabelIssue::NameTooLong);
            }
            if system::ALL.iter().any(|s| s.eq_ignore_ascii_case(name))
                || name.chars().any(char::is_control)
                || name.starts_with('/')
                || name.ends_with('/')
                || name.contains("//")
            {
                out.push(GmailLabelIssue::InvalidName);
            }
        }
        if let Some(color) = &self.color
            && !(is_hex_rgb(&color.background) && is_hex_rgb(&color.text))
        {
            out.push(GmailLabelIssue::InvalidColor);
        }
        out
    }
}

fn is_hex_rgb(text: &str) -> bool {
    text.strip_prefix('#')
        .is_some_and(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A problem with [`GmailLabelSettings`]; the core answers with
/// `invalid_request` and [`Self::message_id`] as `localized`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailLabelIssue {
    /// The name is empty.
    MissingName,
    /// The name is longer than [`MAX_LABEL_NAME`].
    NameTooLong,
    /// A system label's name, control characters or an empty `/` level.
    InvalidName,
    /// A colour is not `#rrggbb`.
    InvalidColor,
    /// Another label of the account has this name.
    DuplicateName,
}

impl GmailLabelIssue {
    /// Every issue.
    pub const ALL: [GmailLabelIssue; 5] = [
        Self::MissingName,
        Self::NameTooLong,
        Self::InvalidName,
        Self::InvalidColor,
        Self::DuplicateName,
    ];

    /// The Fluent id (`gmail-label-issue-*`, in `core-gmail.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::MissingName => "gmail-label-issue-missing-name",
            Self::NameTooLong => "gmail-label-issue-name-too-long",
            Self::InvalidName => "gmail-label-issue-invalid-name",
            Self::InvalidColor => "gmail-label-issue-invalid-color",
            Self::DuplicateName => "gmail-label-issue-duplicate-name",
        }
    }
}

/// Sync state of a label in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GmailLabelSync {
    /// As on the server.
    #[default]
    Synced,
    /// Created or changed locally; the label operation is queued.
    Pending,
}

/// A label of a Gmail account ([`crate::Request::ListGmailLabels`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GmailLabel {
    /// The account.
    pub account_id: AccountId,
    /// Gmail's label id (`INBOX`, `Label_12`); a label created locally
    /// has a temporary `local-…` id until the server assigned one.
    pub id: GmailLabelId,
    /// Name (system labels: Gmail's id; the UI shows its own text).
    pub name: String,
    /// System or user label.
    pub kind: GmailLabelKind,
    /// Colour.
    #[serde(default)]
    pub color: Option<GmailLabelColor>,
    /// Visibility in the label list.
    #[serde(default)]
    pub list_visibility: GmailLabelListVisibility,
    /// Visibility on messages.
    #[serde(default)]
    pub message_visibility: GmailMessageListVisibility,
    /// The Nail tag of a user label (its name).
    #[serde(default)]
    pub tag: Option<String>,
    /// The folder of a system label shown as a folder ([`GmailFolder`]).
    #[serde(default)]
    pub folder_id: Option<FolderId>,
    /// Messages with the label (as stored locally).
    pub total: u32,
    /// Unread ones among them.
    pub unread: u32,
    /// Sync state.
    #[serde(default)]
    pub sync: GmailLabelSync,
}

impl GmailLabel {
    /// The user-editable settings of this label.
    pub fn settings(&self) -> GmailLabelSettings {
        GmailLabelSettings {
            name: self.name.clone(),
            color: self.color.clone(),
            list_visibility: self.list_visibility,
            message_visibility: self.message_visibility,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn folder_follows_label_precedence() {
        assert_eq!(
            GmailFolder::for_labels(&["INBOX", "UNREAD"]),
            GmailFolder::Inbox
        );
        assert_eq!(GmailFolder::for_labels(&["SENT"]), GmailFolder::Sent);
        assert_eq!(
            GmailFolder::for_labels(&["SENT", "INBOX"]),
            GmailFolder::Inbox
        );
        assert_eq!(
            GmailFolder::for_labels(&["INBOX", "TRASH"]),
            GmailFolder::Trash
        );
        assert_eq!(
            GmailFolder::for_labels(&["SPAM", "INBOX"]),
            GmailFolder::Spam
        );
        assert_eq!(GmailFolder::for_labels(&["DRAFT"]), GmailFolder::Drafts);
        assert_eq!(GmailFolder::for_labels(&["Label_1"]), GmailFolder::Archive);
        assert_eq!(GmailFolder::for_labels::<&str>(&[]), GmailFolder::Archive);
        for folder in GmailFolder::ALL {
            assert_eq!(GmailFolder::from_path(folder.path()), Some(folder));
        }
        assert_eq!(GmailFolder::Spam.role(), FolderRole::Junk);
        assert_eq!(GmailFolder::Archive.role(), FolderRole::Archive);
    }

    #[test]
    fn moves_become_label_changes() {
        let archive = GmailFolder::Inbox
            .move_change(GmailFolder::Archive)
            .unwrap();
        assert_eq!(archive.add, Vec::<String>::new());
        assert!(archive.remove.contains(&"INBOX".to_owned()));
        let back = GmailFolder::Archive
            .move_change(GmailFolder::Inbox)
            .unwrap();
        assert_eq!(back.add, ["INBOX"]);
        let spam = GmailFolder::Inbox.move_change(GmailFolder::Spam).unwrap();
        assert_eq!(spam.add, ["SPAM"]);
        assert!(spam.remove.contains(&"INBOX".to_owned()));
        let restore = GmailFolder::Trash.move_change(GmailFolder::Inbox).unwrap();
        assert_eq!(restore.add, ["INBOX"]);
        assert!(restore.remove.contains(&"TRASH".to_owned()));
        let trash = GmailFolder::Sent.move_change(GmailFolder::Trash).unwrap();
        assert_eq!(trash.add, ["TRASH"]);
        assert!(trash.remove.is_empty(), "{:?}", trash.remove);
        assert!(GmailFolder::Inbox.move_change(GmailFolder::Sent).is_none());
        assert!(
            GmailFolder::Archive
                .move_change(GmailFolder::Sent)
                .is_none()
        );
        // Back from Trash / Spam to Sent: the label goes, SENT stays.
        let untrash = GmailFolder::Trash.move_change(GmailFolder::Sent).unwrap();
        assert_eq!(untrash.remove, ["TRASH"]);
        assert!(untrash.add.is_empty(), "{:?}", untrash.add);
        let unspam = GmailFolder::Spam.move_change(GmailFolder::Sent).unwrap();
        assert_eq!(unspam.remove, ["SPAM"]);
        let mut labels = vec!["SENT".to_owned(), "TRASH".to_owned()];
        labels.retain(|l| !untrash.remove.contains(l));
        assert_eq!(GmailFolder::for_labels(&labels), GmailFolder::Sent);
        assert!(
            GmailFolder::Inbox
                .move_change(GmailFolder::Drafts)
                .is_none()
        );
        assert!(
            GmailFolder::Inbox
                .move_change(GmailFolder::Inbox)
                .unwrap()
                .is_empty()
        );
        // Applying a move change to the labels lands in the target folder.
        for from in GmailFolder::ALL {
            for to in [
                GmailFolder::Inbox,
                GmailFolder::Archive,
                GmailFolder::Spam,
                GmailFolder::Trash,
            ] {
                let mut labels: Vec<String> = from.label().into_iter().map(str::to_owned).collect();
                let Some(change) = from.move_change(to) else {
                    assert!(
                        from == GmailFolder::Drafts
                            || (from == GmailFolder::Sent && to == GmailFolder::Archive)
                    );
                    continue;
                };
                labels.retain(|l| !change.remove.contains(l));
                labels.extend(change.add.iter().cloned());
                assert_eq!(GmailFolder::for_labels(&labels), to, "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn flags_map_to_system_labels() {
        let change = LabelChange::from_flags(&FlagChange {
            seen: Some(true),
            flagged: Some(true),
            ..FlagChange::default()
        });
        assert_eq!(change.add, ["STARRED"]);
        assert_eq!(change.remove, ["UNREAD"]);
        let change = LabelChange::from_flags(&FlagChange {
            seen: Some(false),
            flagged: Some(false),
            answered: Some(true),
            ..FlagChange::default()
        });
        assert_eq!(change.add, ["UNREAD"]);
        assert_eq!(change.remove, ["STARRED"]);
        assert!(LabelChange::from_flags(&FlagChange::default()).is_empty());
    }

    #[test]
    fn categories() {
        assert_eq!(
            GmailCategory::for_labels(&["INBOX", "CATEGORY_PROMOTIONS"]),
            Some(GmailCategory::Promotions)
        );
        assert_eq!(GmailCategory::for_labels(&["INBOX"]), None);
        assert_eq!(GmailCategory::parse("Social"), Some(GmailCategory::Social));
        assert_eq!(GmailCategory::parse("spam"), None);
        assert_eq!(
            GmailCategory::Primary.message_id(),
            "gmail-category-primary"
        );
        assert_eq!(
            serde_json::to_value(GmailCategory::Updates).unwrap(),
            json!("updates")
        );
    }

    #[test]
    fn label_settings_issues() {
        assert!(
            GmailLabelSettings::named("Work/Projects")
                .issues()
                .is_empty(),
            "{:?}",
            GmailLabelSettings::named("Work/Projects").issues()
        );
        assert_eq!(
            GmailLabelSettings::named("  ").issues(),
            [GmailLabelIssue::MissingName]
        );
        assert_eq!(
            GmailLabelSettings::named("inbox").issues(),
            [GmailLabelIssue::InvalidName]
        );
        assert_eq!(
            GmailLabelSettings::named("a//b").issues(),
            [GmailLabelIssue::InvalidName]
        );
        assert_eq!(
            GmailLabelSettings::named("x".repeat(226)).issues(),
            [GmailLabelIssue::NameTooLong]
        );
        let mut colored = GmailLabelSettings::named("Red");
        colored.color = Some(GmailLabelColor {
            background: "#fb4c2f".into(),
            text: "#ffffff".into(),
        });
        assert_eq!(colored.issues().len(), 0);
        colored.color = Some(GmailLabelColor {
            background: "red".into(),
            text: "#fff".into(),
        });
        assert_eq!(colored.issues(), [GmailLabelIssue::InvalidColor]);
        for issue in GmailLabelIssue::ALL {
            assert!(issue.message_id().starts_with("gmail-label-issue-"));
        }
    }

    #[test]
    fn poll_range_defaults_and_clamps() {
        assert_eq!(GmailOptions::default().poll_range(), (30, 300));
        let options = GmailOptions {
            poll_interval_secs: Some(1),
            max_poll_interval_secs: Some(5),
        };
        assert_eq!(options.poll_range(), (10, 10));
    }

    #[test]
    fn label_wire_shape() {
        let label: GmailLabel = serde_json::from_value(json!({
            "account_id": "gm", "id": "Label_1", "name": "Work", "kind": "user",
            "tag": "Work", "total": 3, "unread": 1
        }))
        .unwrap();
        assert_eq!(label.list_visibility, GmailLabelListVisibility::Show);
        assert_eq!(label.sync, GmailLabelSync::Synced);
        assert_eq!(label.settings(), GmailLabelSettings::named("Work"));
        let value = serde_json::to_value(GmailLabelListVisibility::ShowIfUnread).unwrap();
        assert_eq!(value, json!("show-if-unread"));
    }
}
