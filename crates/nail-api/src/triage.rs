//! Junk mail, snooze and new-mail notifications (design.md, "Filters &
//! rules" → "Spam", "Notifications", "Snooze"; M2c, ADR 0043).
//!
//! * **Junk**: Nail honours the server's Junk flags and folder and adds a
//!   local Bayesian classifier per account, trained by "Mark as junk /
//!   not junk" ([`crate::Request::MarkJunk`]). It moves suspected junk to
//!   the Junk folder (through the operation queue) and **never deletes**.
//!   Training always happens; [`JunkSettings::enabled`] only switches the
//!   automatic classification of arriving mail.
//! * **Snooze** hides messages until a time: the message gets the
//!   [`SNOOZE_KEYWORD`] keyword and moves to the hidden
//!   [`SNOOZED_FOLDER_NAME`] folder (both through the operation queue);
//!   the core keeps the wake time locally (works offline, survives a
//!   restart) and moves it back when it is due.
//! * **Notifications**: the core decides whether new mail notifies
//!   ([`NotificationSettings`], the rules' `notify` action, quiet hours)
//!   and pushes [`crate::Event::NotificationRequested`]; the desktop shell
//!   shows an OS notification, the remote UI a Web Notification.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::MessageSummary;
use crate::ids::{AccountId, FolderId, MessageId};
use crate::localized::LocalizedMessage;

// ---------------------------------------------------------------------------
// Junk

/// IMAP keyword of messages marked or classified as junk (RFC 5788
/// registry; also set by many servers' filters).
pub const JUNK_KEYWORD: &str = "$Junk";
/// IMAP keyword of messages marked as not junk.
pub const NOT_JUNK_KEYWORD: &str = "$NotJunk";

/// Junk settings of one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct JunkSettings {
    /// Classify arriving mail with the local classifier (off by default:
    /// the classifier is optional). Training by "Mark as junk / not junk"
    /// happens regardless.
    pub enabled: bool,
    /// Probability (percent, 50–99) from which a message counts as junk.
    pub threshold: u8,
    /// Messages of **each** class the classifier must have been trained
    /// with before it classifies anything.
    pub min_training: u32,
    /// Treat the server's `$Junk` / `$NotJunk` keywords and the Junk folder
    /// as verdicts (and train from them).
    pub honor_server_flags: bool,
    /// Move messages classified as junk to the Junk folder (otherwise they
    /// only get the `$Junk` keyword).
    pub move_to_junk: bool,
}

impl Default for JunkSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: Self::DEFAULT_THRESHOLD,
            min_training: Self::DEFAULT_MIN_TRAINING,
            honor_server_flags: true,
            move_to_junk: true,
        }
    }
}

impl JunkSettings {
    /// Default of [`Self::threshold`].
    pub const DEFAULT_THRESHOLD: u8 = 90;
    /// Default of [`Self::min_training`].
    pub const DEFAULT_MIN_TRAINING: u32 = 10;

    /// Problems with the settings (empty = valid).
    pub fn issues(&self) -> Vec<JunkIssue> {
        let mut out = Vec::new();
        if !(50..=99).contains(&self.threshold) {
            out.push(JunkIssue::Threshold);
        }
        if self.min_training == 0 || self.min_training > 10_000 {
            out.push(JunkIssue::MinTraining);
        }
        out
    }
}

/// A problem with [`JunkSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum JunkIssue {
    /// Threshold outside 50–99.
    Threshold,
    /// `min_training` outside 1–10000.
    MinTraining,
}

impl JunkIssue {
    /// All issues.
    pub const ALL: [JunkIssue; 2] = [Self::Threshold, Self::MinTraining];

    /// Fluent message id, e.g. `junk-issue-threshold`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Threshold => "junk-issue-threshold",
            Self::MinTraining => "junk-issue-min-training",
        }
    }
}

/// The classifier of one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct JunkStatus {
    /// The account.
    pub account_id: AccountId,
    /// Its settings.
    pub settings: JunkSettings,
    /// Messages trained as junk.
    pub trained_junk: u32,
    /// Messages trained as not junk.
    pub trained_ham: u32,
    /// Distinct tokens the classifier knows.
    pub tokens: u32,
    /// Whether it has enough training to classify (both counts reach
    /// [`JunkSettings::min_training`]).
    pub ready: bool,
    /// Last training (RFC 3339), `null` = never.
    pub last_trained_at: Option<String>,
}

/// Who decided that a message is (not) junk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum JunkSource {
    /// The server (keyword or delivery into its Junk folder).
    Server,
    /// Nail's classifier.
    Classifier,
    /// The user ("Mark as junk / not junk").
    User,
}

/// The junk verdict of a message, shown by the reader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct JunkVerdict {
    /// Junk or not.
    pub junk: bool,
    /// The classifier's probability in percent (0–100), `null` when it
    /// did not score the message.
    pub score: Option<u8>,
    /// Who decided.
    pub source: JunkSource,
}

// ---------------------------------------------------------------------------
// Snooze

/// IMAP keyword of snoozed messages (lets other clients and a restart
/// recognize them).
pub const SNOOZE_KEYWORD: &str = "$Snoozed";
/// Name of the hidden folder snoozed messages wait in (created on the
/// server when missing; a local folder for POP3 accounts). Hidden from the
/// folder tree.
pub const SNOOZED_FOLDER_NAME: &str = "Snoozed";

/// A snoozed message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SnoozedMessage {
    /// The message (while snoozed it lives in the Snoozed folder).
    pub message: MessageSummary,
    /// When it comes back (RFC 3339).
    pub until: String,
    /// The folder it returns to.
    pub wake_folder_id: FolderId,
    /// When it was snoozed (RFC 3339).
    pub snoozed_at: String,
}

// ---------------------------------------------------------------------------
// Notifications

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}

impl Weekday {
    /// All days, Monday first.
    pub const ALL: [Weekday; 7] = [
        Self::Mon,
        Self::Tue,
        Self::Wed,
        Self::Thu,
        Self::Fri,
        Self::Sat,
        Self::Sun,
    ];
}

/// Times without notifications (local time). A range whose end is before
/// its start crosses midnight (`22:00`–`07:00`); it belongs to the day it
/// starts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuietHours {
    /// Start, `HH:MM` (24 h).
    pub start: String,
    /// End (exclusive), `HH:MM`.
    pub end: String,
    /// Days the range starts on; empty = every day.
    #[serde(default)]
    pub days: Vec<Weekday>,
}

/// Minutes since midnight of an `HH:MM` time, `None` when malformed.
pub fn parse_hhmm(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Which folders notify when no override matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum NotifyFolders {
    /// Only the Inbox (and mail a rule marks with `notify #true`).
    #[default]
    Inbox,
    /// Every synchronized folder except Sent, Drafts, Trash and Junk.
    All,
}

/// What an override applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NotificationScope {
    /// Every folder of a (real) account.
    Account {
        /// The account.
        account_id: AccountId,
    },
    /// One folder.
    Folder {
        /// The folder.
        folder_id: FolderId,
    },
}

/// Notify (or not) for a scope; overrides [`NotificationSettings::folders`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NotificationOverride {
    /// Where it applies.
    pub scope: NotificationScope,
    /// Whether new mail there notifies.
    pub notify: bool,
}

/// New-mail notification settings.
///
/// Precedence (the core's policy, ADR 0043): `enabled` off → nothing;
/// quiet hours → nothing (not deferred); a rule's `notify` action (per
/// rule, in `rules.kdl`) → its choice; a folder override → an account
/// override → [`Self::folders`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct NotificationSettings {
    /// Notifications at all.
    pub enabled: bool,
    /// Show sender and subject (off: only "N new messages").
    pub show_preview: bool,
    /// Play the system sound.
    pub sound: bool,
    /// Folders that notify by default.
    pub folders: NotifyFolders,
    /// Per-account and per-folder overrides (a folder override wins over
    /// its account's).
    pub overrides: Vec<NotificationOverride>,
    /// Quiet hours, `null` = none.
    pub quiet_hours: Option<QuietHours>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            show_preview: true,
            sound: true,
            folders: NotifyFolders::Inbox,
            overrides: Vec::new(),
            quiet_hours: None,
        }
    }
}

impl NotificationSettings {
    /// Problems with the settings (empty = valid).
    pub fn issues(&self) -> Vec<NotificationIssue> {
        let mut out = Vec::new();
        if let Some(q) = &self.quiet_hours {
            match (parse_hhmm(&q.start), parse_hhmm(&q.end)) {
                (Some(s), Some(e)) if s == e => out.push(NotificationIssue::EmptyQuietHours),
                (Some(_), Some(_)) => {}
                _ => out.push(NotificationIssue::QuietHoursTime),
            }
        }
        let mut seen = std::collections::HashSet::new();
        if self.overrides.iter().any(|o| !seen.insert(&o.scope)) {
            out.push(NotificationIssue::DuplicateOverride);
        }
        out
    }
}

/// A problem with [`NotificationSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationIssue {
    /// A quiet-hours time is not `HH:MM`.
    QuietHoursTime,
    /// Quiet hours start and end at the same time.
    EmptyQuietHours,
    /// Two overrides for the same account or folder.
    DuplicateOverride,
}

impl NotificationIssue {
    /// All issues.
    pub const ALL: [NotificationIssue; 3] = [
        Self::QuietHoursTime,
        Self::EmptyQuietHours,
        Self::DuplicateOverride,
    ];

    /// Fluent message id, e.g. `notification-issue-quiet-hours-time`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::QuietHoursTime => "notification-issue-quiet-hours-time",
            Self::EmptyQuietHours => "notification-issue-empty-quiet-hours",
            Self::DuplicateOverride => "notification-issue-duplicate-override",
        }
    }
}

/// Why a notification was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationKind {
    /// New mail arrived.
    NewMail,
    /// Snoozed mail came back.
    SnoozeWoke,
    /// [`crate::Request::TestNotification`].
    Test,
}

/// A notification the UI (or the desktop shell) should show. The texts
/// are [`LocalizedMessage`]s; sender and subject are arguments of `body`
/// (absent when [`NotificationSettings::show_preview`] is off).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MailNotification {
    /// Unique id (a later notification with the same `tag` replaces it).
    pub id: String,
    /// Grouping tag, e.g. per folder, so a burst shows as one notification.
    pub tag: String,
    /// Why.
    pub kind: NotificationKind,
    /// The account.
    pub account_id: Option<AccountId>,
    /// The folder.
    pub folder_id: Option<FolderId>,
    /// The messages it is about, newest first.
    pub message_ids: Vec<MessageId>,
    /// Title, e.g. `notification-new-mail-title` with `count` and `account`.
    pub title: LocalizedMessage,
    /// Body, e.g. `notification-new-mail-body` with `sender` and `subject`;
    /// `null` without preview.
    pub body: Option<LocalizedMessage>,
    /// Play the system sound.
    pub sound: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn junk_defaults_and_issues() {
        let s: JunkSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s, JunkSettings::default());
        assert!(!s.enabled && s.honor_server_flags && s.move_to_junk);
        assert!(s.issues().is_empty());
        let bad = JunkSettings {
            threshold: 100,
            min_training: 0,
            ..JunkSettings::default()
        };
        assert_eq!(bad.issues(), [JunkIssue::Threshold, JunkIssue::MinTraining]);
        assert_eq!(
            JunkIssue::MinTraining.message_id(),
            "junk-issue-min-training"
        );
        let verdict = JunkVerdict {
            junk: true,
            score: Some(97),
            source: JunkSource::Classifier,
        };
        assert_eq!(
            serde_json::to_value(&verdict).unwrap(),
            json!({"junk": true, "score": 97, "source": "classifier"})
        );
    }

    #[test]
    fn quiet_hours_and_notification_issues() {
        assert_eq!(parse_hhmm("22:00"), Some(1320));
        assert_eq!(parse_hhmm("07:05"), Some(425));
        for bad in ["24:00", "7:00", "07:60", "0700", ""] {
            assert_eq!(parse_hhmm(bad), None, "{bad}");
        }
        let s: NotificationSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(s, NotificationSettings::default());
        assert!(s.enabled && s.issues().is_empty());
        let over = |id: &str| NotificationOverride {
            scope: NotificationScope::Folder {
                folder_id: id.into(),
            },
            notify: false,
        };
        let bad = NotificationSettings {
            quiet_hours: Some(QuietHours {
                start: "22:00".into(),
                end: "22:00".into(),
                days: vec![],
            }),
            overrides: vec![over("f1"), over("f1")],
            ..NotificationSettings::default()
        };
        assert_eq!(
            bad.issues(),
            [
                NotificationIssue::EmptyQuietHours,
                NotificationIssue::DuplicateOverride
            ]
        );
        let malformed = NotificationSettings {
            quiet_hours: Some(QuietHours {
                start: "22".into(),
                end: "07:00".into(),
                days: vec![Weekday::Sat],
            }),
            ..NotificationSettings::default()
        };
        assert_eq!(malformed.issues(), [NotificationIssue::QuietHoursTime]);
        assert_eq!(
            serde_json::to_value(over("f1")).unwrap(),
            json!({"scope": {"type": "folder", "folder_id": "f1"}, "notify": false})
        );
        assert_eq!(serde_json::to_value(Weekday::Mon).unwrap(), json!("mon"));
    }

    #[test]
    fn notifications_serialize() {
        let n = MailNotification {
            id: "n1".into(),
            tag: "work/inbox".into(),
            kind: NotificationKind::NewMail,
            account_id: Some("work".into()),
            folder_id: Some("f1".into()),
            message_ids: vec!["m1".into()],
            title: LocalizedMessage::new("notification-new-mail-title").arg("count", 1u32),
            body: None,
            sound: true,
        };
        let value = serde_json::to_value(&n).unwrap();
        assert_eq!(value["kind"], json!("new-mail"));
        assert_eq!(value["title"]["args"]["count"], json!(1));
        assert_eq!(
            serde_json::from_value::<MailNotification>(value).unwrap(),
            n
        );
    }
}
